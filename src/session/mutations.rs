//! Session mutation helpers.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::session::history::{entries_to_session_info, filter_transcript_entries};
use crate::session::paths::{get_projects_dir, project_key_for_directory, validate_uuid};
use crate::session::{SessionKey, SessionStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkSessionResult {
    pub session_id: String,
}

pub fn rename_session(
    session_id: &str,
    title: &str,
    directory: Option<&Path>,
) -> std::io::Result<()> {
    validate_session_id(session_id)?;
    let title = title.trim();
    if title.is_empty() {
        return Err(invalid_input("title must be non-empty"));
    }
    append_to_session(
        session_id,
        directory,
        json!({"type":"custom-title","customTitle":title,"sessionId":session_id}),
    )
}

pub fn tag_session(
    session_id: &str,
    tag: Option<&str>,
    directory: Option<&Path>,
) -> std::io::Result<()> {
    validate_session_id(session_id)?;
    let tag = match tag {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(invalid_input("tag must be non-empty (use None to clear)"));
            }
            trimmed
        }
        None => "",
    };
    append_to_session(
        session_id,
        directory,
        json!({"type":"tag","tag":tag,"sessionId":session_id}),
    )
}

pub fn delete_session(session_id: &str, directory: Option<&Path>) -> std::io::Result<()> {
    validate_session_id(session_id)?;
    let path = find_session_file(session_id, directory)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "session not found"))?;
    std::fs::remove_file(&path)?;
    let _ = std::fs::remove_dir_all(path.with_extension(""));
    Ok(())
}

pub fn fork_session(
    session_id: &str,
    directory: Option<&Path>,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
) -> std::io::Result<ForkSessionResult> {
    validate_session_id(session_id)?;
    if let Some(id) = up_to_message_id {
        validate_session_id(id)?;
    }
    let path = find_session_file(session_id, directory)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "session not found"))?;
    let entries = read_jsonl_file(&path);
    let (new_id, forked) = build_fork_entries(&entries, session_id, up_to_message_id, title)?;
    let out_path = path.with_file_name(format!("{new_id}.jsonl"));
    write_jsonl_file(&out_path, &forked)?;
    Ok(ForkSessionResult { session_id: new_id })
}

pub async fn rename_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    title: &str,
    directory: Option<&Path>,
) -> Result<(), StoreError> {
    if validate_uuid(session_id).is_none() {
        return Err(StoreError::Adapter(format!(
            "invalid session_id: {session_id}"
        )));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err(StoreError::Adapter("title must be non-empty".into()));
    }
    append_meta_to_store(
        store,
        session_id,
        directory,
        json!({"type":"custom-title","customTitle":title,"sessionId":session_id}),
    )
    .await
}

pub async fn tag_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    tag: Option<&str>,
    directory: Option<&Path>,
) -> Result<(), StoreError> {
    if validate_uuid(session_id).is_none() {
        return Err(StoreError::Adapter(format!(
            "invalid session_id: {session_id}"
        )));
    }
    let tag = tag.map(str::trim).unwrap_or("");
    append_meta_to_store(
        store,
        session_id,
        directory,
        json!({"type":"tag","tag":tag,"sessionId":session_id}),
    )
    .await
}

pub async fn delete_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
) -> Result<(), StoreError> {
    if validate_uuid(session_id).is_none() {
        return Err(StoreError::Adapter(format!(
            "invalid session_id: {session_id}"
        )));
    }
    store
        .delete(SessionKey {
            project_key: project_key_for_directory(directory.unwrap_or(Path::new("."))),
            session_id: session_id.into(),
            subpath: None,
        })
        .await
}

pub async fn fork_session_via_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
) -> Result<ForkSessionResult, StoreError> {
    if validate_uuid(session_id).is_none() {
        return Err(StoreError::Adapter(format!(
            "invalid session_id: {session_id}"
        )));
    }
    let project_key = project_key_for_directory(directory.unwrap_or(Path::new(".")));
    let entries = store
        .load(SessionKey {
            project_key: project_key.clone(),
            session_id: session_id.into(),
            subpath: None,
        })
        .await?
        .ok_or_else(|| StoreError::Adapter("session not found".into()))?;
    let (new_id, forked) = build_fork_entries(&entries, session_id, up_to_message_id, title)
        .map_err(|e| StoreError::Adapter(e.to_string()))?;
    store
        .append(
            SessionKey {
                project_key,
                session_id: new_id.clone(),
                subpath: None,
            },
            forked,
        )
        .await?;
    Ok(ForkSessionResult { session_id: new_id })
}

pub(crate) fn find_session_file(session_id: &str, directory: Option<&Path>) -> Option<PathBuf> {
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

fn append_to_session(
    session_id: &str,
    directory: Option<&Path>,
    value: Value,
) -> std::io::Result<()> {
    let path = find_session_file(session_id, directory)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "session not found"))?;
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    use std::io::Write;
    writeln!(file, "{}", serde_json::to_string(&value).unwrap())?;
    Ok(())
}

async fn append_meta_to_store(
    store: &dyn SessionStore,
    session_id: &str,
    directory: Option<&Path>,
    value: Value,
) -> Result<(), StoreError> {
    store
        .append(
            SessionKey {
                project_key: project_key_for_directory(directory.unwrap_or(Path::new("."))),
                session_id: session_id.into(),
                subpath: None,
            },
            vec![value],
        )
        .await
}

fn build_fork_entries(
    entries: &[Value],
    source_session_id: &str,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
) -> std::io::Result<(String, Vec<Value>)> {
    let transcript = filter_transcript_entries(entries.to_vec());
    if transcript.is_empty() {
        return Err(invalid_input("session has no messages to fork"));
    }
    if let Some(id) = up_to_message_id {
        if !transcript
            .iter()
            .any(|e| e.get("uuid").and_then(Value::as_str) == Some(id))
        {
            return Err(invalid_input("up_to_message_id not found"));
        }
    }
    let new_id = uuid::Uuid::new_v4().to_string();
    let mut id_map = std::collections::HashMap::new();
    let mut out = Vec::new();
    for entry in transcript {
        let uuid = entry
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let new_uuid = uuid::Uuid::new_v4().to_string();
        id_map.insert(uuid.clone(), new_uuid.clone());
        let mut entry = entry;
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("uuid".into(), Value::String(new_uuid));
            obj.insert("sessionId".into(), Value::String(new_id.clone()));
            if obj.get("session_id").is_some() {
                obj.insert("session_id".into(), Value::String(new_id.clone()));
            }
            if let Some(parent) = obj
                .get("parentUuid")
                .and_then(Value::as_str)
                .map(String::from)
            {
                if let Some(mapped) = id_map.get(&parent) {
                    obj.insert("parentUuid".into(), Value::String(mapped.clone()));
                }
            }
        }
        let stop = up_to_message_id == Some(uuid.as_str());
        out.push(entry);
        if stop {
            break;
        }
    }
    let derived_title = title.map(str::to_string).or_else(|| {
        entries_to_session_info(source_session_id, entries, 0, None)
            .map(|i| format!("{} (fork)", i.summary))
    });
    if let Some(title) = derived_title {
        out.push(json!({"type":"custom-title","customTitle":title,"sessionId":new_id}));
    }
    Ok((new_id, out))
}

pub(crate) fn read_jsonl_file(path: &Path) -> Vec<Value> {
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

pub(crate) fn write_jsonl_file(path: &Path, entries: &[Value]) -> std::io::Result<()> {
    let mut out = String::new();
    for entry in entries {
        out.push_str(&serde_json::to_string(entry).unwrap());
        out.push('\n');
    }
    std::fs::write(path, out)
}

fn validate_session_id(session_id: &str) -> std::io::Result<()> {
    if validate_uuid(session_id).is_some() {
        Ok(())
    } else {
        Err(invalid_input(format!("invalid session_id: {session_id}")))
    }
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}
