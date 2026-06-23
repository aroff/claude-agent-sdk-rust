//! Session discovery and transcript reading.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::paths::{get_projects_dir, project_key_for_directory, validate_uuid};
use crate::session::{SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SDKSessionInfo {
    pub session_id: String,
    pub summary: String,
    pub last_modified: i64,
    pub file_size: Option<u64>,
    pub custom_title: Option<String>,
    pub first_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub tag: Option<String>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMessage {
    pub r#type: String,
    pub uuid: String,
    pub session_id: String,
    pub message: Value,
    pub parent_tool_use_id: Option<Value>,
}

pub fn list_sessions(
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
    _include_worktrees: bool,
) -> Vec<SDKSessionInfo> {
    let mut infos = if let Some(dir) = directory {
        let project_dir = get_projects_dir(None).join(project_key_for_directory(dir));
        read_sessions_from_project_dir(&project_dir, Some(dir))
    } else {
        let projects_dir = get_projects_dir(None);
        std::fs::read_dir(projects_dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.flatten())
            .filter(|e| e.path().is_dir())
            .flat_map(|e| read_sessions_from_project_dir(&e.path(), None))
            .collect()
    };
    sort_page(&mut infos, limit, offset)
}

pub fn get_session_info(session_id: &str, directory: Option<&Path>) -> Option<SDKSessionInfo> {
    validate_uuid(session_id)?;
    find_session_file(session_id, directory).and_then(|path| parse_session_info_file(&path, None))
}

pub fn get_session_messages(
    session_id: &str,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    if validate_uuid(session_id).is_none() {
        return Vec::new();
    }
    let Some(path) = find_session_file(session_id, directory) else {
        return Vec::new();
    };
    let entries = read_jsonl_file(&path);
    entries_to_session_messages(filter_transcript_entries(entries), limit, offset)
}

pub fn list_subagents(session_id: &str, directory: Option<&Path>) -> Vec<String> {
    if validate_uuid(session_id).is_none() {
        return Vec::new();
    }
    let Some(dir) = resolve_subagents_dir(session_id, directory) else {
        return Vec::new();
    };
    collect_agent_files(&dir)
        .into_iter()
        .map(|(agent_id, _)| agent_id)
        .collect()
}

pub fn get_subagent_messages(
    session_id: &str,
    agent_id: &str,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    if validate_uuid(session_id).is_none() || agent_id.is_empty() {
        return Vec::new();
    }
    let Some(dir) = resolve_subagents_dir(session_id, directory) else {
        return Vec::new();
    };
    let Some((_, path)) = collect_agent_files(&dir)
        .into_iter()
        .find(|(found_id, _)| found_id == agent_id)
    else {
        return Vec::new();
    };
    entries_to_subagent_messages(
        filter_transcript_entries(read_jsonl_file(&path)),
        limit,
        offset,
    )
}

pub async fn list_sessions_from_store(
    store: &dyn SessionStore,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SDKSessionInfo>, crate::session::StoreError> {
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let mut infos = match store.list_session_summaries(&project_key).await {
        Ok(summaries) => summaries
            .into_iter()
            .filter_map(|s| summary_value_to_info(s.session_id, s.mtime, s.data, None))
            .collect(),
        Err(crate::session::StoreError::NotImplemented) => {
            let listing = store.list_sessions(&project_key).await?;
            let mut out = Vec::new();
            for entry in listing {
                let key = SessionKey {
                    project_key: project_key.clone(),
                    session_id: entry.session_id.clone(),
                    subpath: None,
                };
                if let Some(entries) = store.load(key).await? {
                    if let Some(info) =
                        entries_to_session_info(&entry.session_id, &entries, entry.mtime, None)
                    {
                        out.push(info);
                    }
                }
            }
            out
        }
        Err(e) => return Err(e),
    };
    Ok(sort_page(&mut infos, limit, offset))
}

pub async fn get_session_info_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
) -> Result<Option<SDKSessionInfo>, crate::session::StoreError> {
    if validate_uuid(session_id).is_none() {
        return Ok(None);
    }
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let key = SessionKey {
        project_key,
        session_id: session_id.into(),
        subpath: None,
    };
    Ok(store.load(key).await?.and_then(|entries| {
        entries_to_session_info(session_id, &entries, mtime_from_entries(&entries), None)
    }))
}

pub async fn get_session_messages_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SessionMessage>, crate::session::StoreError> {
    if validate_uuid(session_id).is_none() {
        return Ok(Vec::new());
    }
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let key = SessionKey {
        project_key,
        session_id: session_id.into(),
        subpath: None,
    };
    Ok(store
        .load(key)
        .await?
        .map(|entries| {
            entries_to_session_messages(filter_transcript_entries(entries), limit, offset)
        })
        .unwrap_or_default())
}

pub async fn list_subagents_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
) -> Result<Vec<String>, crate::session::StoreError> {
    if validate_uuid(session_id).is_none() {
        return Ok(Vec::new());
    }
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let subkeys = store
        .list_subkeys(SessionListSubkeysKey {
            project_key,
            session_id: session_id.into(),
        })
        .await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for subkey in subkeys {
        let Some(last) = subkey.rsplit('/').next() else {
            continue;
        };
        if let Some(agent_id) = last.strip_prefix("agent-") {
            if seen.insert(agent_id.to_string()) {
                out.push(agent_id.to_string());
            }
        }
    }
    Ok(out)
}

pub async fn get_subagent_messages_from_store(
    store: &dyn SessionStore,
    session_id: &str,
    agent_id: &str,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<SessionMessage>, crate::session::StoreError> {
    if validate_uuid(session_id).is_none() || agent_id.is_empty() {
        return Ok(Vec::new());
    }
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let subkeys = store
        .list_subkeys(SessionListSubkeysKey {
            project_key: project_key.clone(),
            session_id: session_id.into(),
        })
        .await
        .unwrap_or_default();
    let subpath = subkeys
        .into_iter()
        .find(|s| s.rsplit('/').next() == Some(&format!("agent-{agent_id}")))
        .unwrap_or_else(|| format!("subagents/agent-{agent_id}"));
    let key = SessionKey {
        project_key,
        session_id: session_id.into(),
        subpath: Some(subpath),
    };
    Ok(store
        .load(key)
        .await?
        .map(|entries| {
            entries_to_subagent_messages(filter_transcript_entries(entries), limit, offset)
        })
        .unwrap_or_default())
}

pub(crate) fn filter_transcript_entries(entries: Vec<SessionStoreEntry>) -> Vec<Value> {
    entries
        .into_iter()
        .filter(|e| {
            matches!(
                e.get("type").and_then(Value::as_str),
                Some("user" | "assistant" | "progress" | "system" | "attachment")
            ) && e.get("uuid").and_then(Value::as_str).is_some()
        })
        .collect()
}

pub(crate) fn entries_to_session_messages(
    entries: Vec<Value>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    let visible: Vec<_> = build_conversation_chain(entries)
        .into_iter()
        .filter(is_visible_message)
        .filter_map(to_session_message)
        .collect();
    page(visible, limit, offset)
}

pub(crate) fn entries_to_subagent_messages(
    entries: Vec<Value>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<SessionMessage> {
    let visible: Vec<_> = build_subagent_chain(entries)
        .into_iter()
        .filter(|e| {
            matches!(
                e.get("type").and_then(Value::as_str),
                Some("user" | "assistant")
            )
        })
        .filter_map(to_session_message)
        .collect();
    page(visible, limit, offset)
}

fn read_sessions_from_project_dir(
    project_dir: &Path,
    directory: Option<&Path>,
) -> Vec<SDKSessionInfo> {
    std::fs::read_dir(project_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|e| {
            let path = e.path();
            (path.extension().and_then(|e| e.to_str()) == Some("jsonl"))
                .then(|| parse_session_info_file(&path, directory))
                .flatten()
        })
        .collect()
}

fn parse_session_info_file(path: &Path, directory: Option<&Path>) -> Option<SDKSessionInfo> {
    let session_id = path.file_stem()?.to_str()?.to_string();
    validate_uuid(&session_id)?;
    let metadata = std::fs::metadata(path).ok();
    let entries = read_jsonl_file(path);
    entries_to_session_info(
        &session_id,
        &entries,
        metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(system_time_ms)
            .unwrap_or_else(|| mtime_from_entries(&entries)),
        metadata.as_ref().map(|m| m.len()).or(Some(0)),
    )
    .map(|mut info| {
        if info.cwd.is_none() {
            info.cwd = directory.map(|d| d.to_string_lossy().into_owned());
        }
        info
    })
}

pub(crate) fn entries_to_session_info(
    session_id: &str,
    entries: &[Value],
    last_modified: i64,
    file_size: Option<u64>,
) -> Option<SDKSessionInfo> {
    if entries
        .first()
        .and_then(|e| e.get("isSidechain"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return None;
    }
    let mut custom_title = None;
    let mut ai_title = None;
    let mut first_prompt = None;
    let mut git_branch = None;
    let mut cwd = None;
    let mut tag = None;
    let mut created_at = None;
    for entry in entries {
        if created_at.is_none() {
            created_at = entry
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_iso_epoch_ms);
        }
        match entry.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                custom_title = entry
                    .get("customTitle")
                    .and_then(Value::as_str)
                    .map(String::from);
            }
            Some("tag") => {
                tag = entry
                    .get("tag")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(String::from);
            }
            Some("summary") => {
                ai_title = entry
                    .get("summary")
                    .or_else(|| entry.get("aiTitle"))
                    .and_then(Value::as_str)
                    .map(String::from);
            }
            Some("user") => {
                if first_prompt.is_none() {
                    first_prompt = extract_message_text(entry.get("message"));
                }
                cwd = cwd.or_else(|| entry.get("cwd").and_then(Value::as_str).map(String::from));
                git_branch = git_branch.or_else(|| {
                    entry
                        .get("gitBranch")
                        .and_then(Value::as_str)
                        .map(String::from)
                });
            }
            _ => {}
        }
    }
    let summary = custom_title
        .clone()
        .or(ai_title)
        .or_else(|| first_prompt.clone())
        .unwrap_or_default();
    if summary.is_empty() {
        return None;
    }
    Some(SDKSessionInfo {
        session_id: session_id.into(),
        summary,
        last_modified,
        file_size,
        custom_title,
        first_prompt,
        git_branch,
        cwd,
        tag,
        created_at,
    })
}

fn summary_value_to_info(
    session_id: String,
    mtime: i64,
    data: Value,
    file_size: Option<u64>,
) -> Option<SDKSessionInfo> {
    let obj = data.as_object()?;
    Some(SDKSessionInfo {
        session_id,
        summary: obj
            .get("summary")
            .or_else(|| obj.get("custom_title"))
            .or_else(|| obj.get("first_prompt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        last_modified: mtime,
        file_size,
        custom_title: obj
            .get("custom_title")
            .and_then(Value::as_str)
            .map(String::from),
        first_prompt: obj
            .get("first_prompt")
            .and_then(Value::as_str)
            .map(String::from),
        git_branch: obj
            .get("git_branch")
            .and_then(Value::as_str)
            .map(String::from),
        cwd: obj.get("cwd").and_then(Value::as_str).map(String::from),
        tag: obj.get("tag").and_then(Value::as_str).map(String::from),
        created_at: obj.get("created_at").and_then(Value::as_i64),
    })
    .filter(|i| !i.summary.is_empty())
}

fn find_session_file(session_id: &str, directory: Option<&Path>) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    if let Some(dir) = directory {
        let path = get_projects_dir(None)
            .join(project_key_for_directory(dir))
            .join(file_name);
        return path.exists().then_some(path);
    }
    std::fs::read_dir(get_projects_dir(None))
        .ok()?
        .flatten()
        .map(|e| e.path().join(&file_name))
        .find(|p| p.exists())
}

fn resolve_subagents_dir(session_id: &str, directory: Option<&Path>) -> Option<PathBuf> {
    find_session_file(session_id, directory).map(|p| p.with_extension("").join("subagents"))
}

fn collect_agent_files(base: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(agent_id) = name
                    .strip_prefix("agent-")
                    .and_then(|s| s.strip_suffix(".jsonl"))
                {
                    out.push((agent_id.to_string(), path));
                }
            }
        }
    }
    walk(base, &mut out);
    out
}

fn read_jsonl_file(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .ok()
        .map(|content| {
            content
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn build_conversation_chain(entries: Vec<Value>) -> Vec<Value> {
    let mut by_uuid: HashMap<String, Value> = HashMap::new();
    let mut parent_uuids = HashSet::new();
    for entry in &entries {
        if let Some(uuid) = entry.get("uuid").and_then(Value::as_str) {
            by_uuid.insert(uuid.to_string(), entry.clone());
        }
        if let Some(parent) = entry.get("parentUuid").and_then(Value::as_str) {
            parent_uuids.insert(parent.to_string());
        }
    }
    let leaf = entries
        .iter()
        .rev()
        .find(|e| {
            e.get("uuid")
                .and_then(Value::as_str)
                .map(|uuid| !parent_uuids.contains(uuid))
                .unwrap_or(false)
                && matches!(
                    e.get("type").and_then(Value::as_str),
                    Some("user" | "assistant")
                )
                && !e
                    .get("isSidechain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                && !e.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
        })
        .cloned();
    build_parent_chain(leaf, &by_uuid)
}

fn build_subagent_chain(entries: Vec<Value>) -> Vec<Value> {
    let mut by_uuid: HashMap<String, Value> = HashMap::new();
    for entry in &entries {
        if let Some(uuid) = entry.get("uuid").and_then(Value::as_str) {
            by_uuid.insert(uuid.to_string(), entry.clone());
        }
    }
    let leaf = entries
        .iter()
        .rev()
        .find(|e| {
            matches!(
                e.get("type").and_then(Value::as_str),
                Some("user" | "assistant")
            )
        })
        .cloned();
    build_parent_chain(leaf, &by_uuid)
}

fn build_parent_chain(leaf: Option<Value>, by_uuid: &HashMap<String, Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = leaf;
    while let Some(entry) = current {
        let Some(uuid) = entry.get("uuid").and_then(Value::as_str) else {
            break;
        };
        if !seen.insert(uuid.to_string()) {
            break;
        }
        let parent = entry
            .get("parentUuid")
            .and_then(Value::as_str)
            .and_then(|p| by_uuid.get(p).cloned());
        out.push(entry);
        current = parent;
    }
    out.reverse();
    out
}

fn is_visible_message(entry: &Value) -> bool {
    matches!(
        entry.get("type").and_then(Value::as_str),
        Some("user" | "assistant")
    ) && !entry
        .get("isMeta")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !entry
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && entry.get("teamName").is_none()
}

fn to_session_message(entry: Value) -> Option<SessionMessage> {
    Some(SessionMessage {
        r#type: entry.get("type")?.as_str()?.to_string(),
        uuid: entry.get("uuid")?.as_str()?.to_string(),
        session_id: entry
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        message: entry.get("message").cloned().unwrap_or(Value::Null),
        parent_tool_use_id: None,
    })
}

fn extract_message_text(message: Option<&Value>) -> Option<String> {
    let message = message?;
    let content = message.get("content").unwrap_or(message);
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(arr) => arr.iter().find_map(|b| {
            b.get("text")
                .and_then(Value::as_str)
                .or_else(|| b.as_str())
                .map(String::from)
        }),
        _ => None,
    }
}

fn mtime_from_entries(entries: &[Value]) -> i64 {
    entries
        .iter()
        .rev()
        .find_map(|e| {
            e.get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_iso_epoch_ms)
        })
        .unwrap_or(0)
}

fn parse_iso_epoch_ms(s: &str) -> Option<i64> {
    let date_time = s.split(['Z', '+']).next().unwrap_or(s);
    let mut parts = date_time.split('T');
    let date = parts.next()?;
    let time = parts.next().unwrap_or("00:00:00");
    let mut d = date.split('-').filter_map(|p| p.parse::<i64>().ok());
    let y = d.next()?;
    let m = d.next()?;
    let day = d.next()?;
    let mut t = time
        .split(':')
        .filter_map(|p| p.split('.').next()?.parse::<i64>().ok());
    let h = t.next().unwrap_or(0);
    let min = t.next().unwrap_or(0);
    let sec = t.next().unwrap_or(0);
    Some((((days_from_civil(y, m, day) * 24 + h) * 60 + min) * 60 + sec) * 1000)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

fn sort_page(
    infos: &mut [SDKSessionInfo],
    limit: Option<usize>,
    offset: usize,
) -> Vec<SDKSessionInfo> {
    infos.sort_by_key(|i| std::cmp::Reverse(i.last_modified));
    page(infos.to_vec(), limit, offset)
}

fn page<T: Clone>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let iter = items.into_iter().skip(offset);
    if let Some(limit) = limit.filter(|l| *l > 0) {
        iter.take(limit).collect()
    } else {
        iter.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{InMemorySessionStore, SessionStore};
    use serde_json::json;

    #[tokio::test]
    async fn store_messages_follow_parent_chain() {
        let store = InMemorySessionStore::new();
        let sid = "550e8400-e29b-41d4-a716-446655440000";
        let key = SessionKey {
            project_key: project_key_for_directory(Path::new(".")),
            session_id: sid.into(),
            subpath: None,
        };
        store
            .append(
                key,
                vec![
                    json!({"type":"user","uuid":"u1","sessionId":sid,"message":{"content":"hi"}}),
                    json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":sid,"message":{"content":[{"type":"text","text":"hello"}]}}),
                ],
            )
            .await
            .unwrap();
        let messages = get_session_messages_from_store(&store, sid, None, None, 0)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].r#type, "user");
    }
}
