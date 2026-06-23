//! Store-backed resume materialization.
//!
//! Loads a session from a [`SessionStore`] into a temporary Claude config
//! directory so the subprocess can resume it through the CLI's normal
//! transcript loading path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use tracing::warn;

use crate::error::ClaudeSdkError;
use crate::options::ClaudeAgentOptions;
use crate::session::batcher::TranscriptMirrorBatcher;
use crate::session::paths::{get_projects_dir, project_key_for_directory, validate_uuid};
use crate::session::{
    SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry, SessionStoreFlushMode,
};

/// A materialized store-backed resume directory.
#[derive(Debug, Clone)]
pub struct MaterializedResume {
    pub config_dir: PathBuf,
    pub resume_session_id: String,
}

impl MaterializedResume {
    /// Best-effort removal of the temporary config directory.
    pub async fn cleanup(&self) {
        remove_dir_all_retry(&self.config_dir).await;
    }
}

/// Return an options copy pointed at the materialized config directory.
pub fn apply_materialized_options(
    options: &ClaudeAgentOptions,
    materialized: &MaterializedResume,
) -> ClaudeAgentOptions {
    let mut options = options.clone();
    options.env.insert(
        "CLAUDE_CONFIG_DIR".into(),
        materialized.config_dir.to_string_lossy().into_owned(),
    );
    options.resume = Some(materialized.resume_session_id.clone());
    options.continue_conversation = false;
    options
}

/// Build the transcript mirror batcher with path resolution matching the
/// subprocess's effective Claude config directory.
pub fn build_mirror_batcher(
    store: Arc<dyn SessionStore>,
    materialized: Option<&MaterializedResume>,
    env: Option<&BTreeMap<String, String>>,
    flush_mode: SessionStoreFlushMode,
    on_error: Arc<dyn Fn(SessionKey, String) + Send + Sync>,
) -> TranscriptMirrorBatcher {
    let projects_dir = materialized
        .map(|m| m.config_dir.join("projects"))
        .unwrap_or_else(|| get_projects_dir(env));
    match flush_mode {
        SessionStoreFlushMode::Batched => {
            TranscriptMirrorBatcher::new(store, projects_dir.to_string_lossy(), on_error)
        }
        SessionStoreFlushMode::Eager => {
            TranscriptMirrorBatcher::new_eager(store, projects_dir.to_string_lossy(), on_error)
        }
    }
}

/// Materialize a resume target from `options.session_store`, if one is needed.
pub async fn materialize_resume_session(
    options: &ClaudeAgentOptions,
) -> Result<Option<MaterializedResume>, ClaudeSdkError> {
    let Some(store) = options.session_store.clone() else {
        return Ok(None);
    };
    if options.resume.is_none() && !options.continue_conversation {
        return Ok(None);
    }

    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let project_key = project_key_for_directory(&cwd);
    let timeout = Duration::from_millis(options.load_timeout_ms);

    let resolved = if let Some(session_id) = &options.resume {
        if validate_uuid(session_id).is_none() {
            return Ok(None);
        }
        load_candidate(store.as_ref(), &project_key, session_id, timeout).await?
    } else {
        resolve_continue_candidate(store.as_ref(), &project_key, timeout).await?
    };

    let Some((session_id, entries)) = resolved else {
        return Ok(None);
    };

    let tmp_base = make_temp_config_dir()?;
    let result = async {
        let project_dir = tmp_base.join("projects").join(&project_key);
        tokio::fs::create_dir_all(&project_dir)
            .await
            .map_err(|e| ClaudeSdkError::new(format!("failed to create resume dir: {e}")))?;
        write_jsonl(&project_dir.join(format!("{session_id}.jsonl")), &entries).await?;
        copy_auth_files(&tmp_base, &options.env).await;
        materialize_subkeys(
            store.as_ref(),
            &tmp_base,
            &project_dir,
            &project_key,
            &session_id,
            timeout,
        )
        .await?;
        Ok::<(), ClaudeSdkError>(())
    }
    .await;

    if let Err(e) = result {
        remove_dir_all_retry(&tmp_base).await;
        return Err(e);
    }

    Ok(Some(MaterializedResume {
        config_dir: tmp_base,
        resume_session_id: session_id,
    }))
}

async fn load_candidate(
    store: &dyn SessionStore,
    project_key: &str,
    session_id: &str,
    timeout: Duration,
) -> Result<Option<(String, Vec<SessionStoreEntry>)>, ClaudeSdkError> {
    let key = SessionKey {
        project_key: project_key.into(),
        session_id: session_id.into(),
        subpath: None,
    };
    let entries = timeout_store(
        store.load(key),
        timeout,
        &format!("SessionStore.load() for session {session_id}"),
    )
    .await?;
    Ok(entries
        .filter(|entries| !entries.is_empty())
        .map(|entries| (session_id.to_string(), entries)))
}

async fn resolve_continue_candidate(
    store: &dyn SessionStore,
    project_key: &str,
    timeout: Duration,
) -> Result<Option<(String, Vec<SessionStoreEntry>)>, ClaudeSdkError> {
    let mut sessions = timeout_store(
        store.list_sessions(project_key),
        timeout,
        "SessionStore.list_sessions()",
    )
    .await?;
    sessions.sort_by_key(|s| std::cmp::Reverse(s.mtime));

    for cand in sessions {
        if validate_uuid(&cand.session_id).is_none() {
            continue;
        }
        let Some(loaded) = load_candidate(store, project_key, &cand.session_id, timeout).await?
        else {
            continue;
        };
        let is_sidechain = loaded
            .1
            .first()
            .and_then(Value::as_object)
            .and_then(|m| m.get("isSidechain"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_sidechain {
            return Ok(Some(loaded));
        }
    }
    Ok(None)
}

async fn timeout_store<T, F>(future: F, timeout: Duration, what: &str) -> Result<T, ClaudeSdkError>
where
    F: std::future::Future<Output = Result<T, crate::session::StoreError>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(ClaudeSdkError::new(format!(
            "{what} failed during resume materialization: {e}"
        ))),
        Err(_) => Err(ClaudeSdkError::new(format!(
            "{what} timed out after {}ms during resume materialization",
            timeout.as_millis()
        ))),
    }
}

fn make_temp_config_dir() -> Result<PathBuf, ClaudeSdkError> {
    let dir = std::env::temp_dir().join(format!("claude-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| ClaudeSdkError::new(format!("failed to create temp resume dir: {e}")))?;
    Ok(dir)
}

async fn write_jsonl(path: &Path, entries: &[SessionStoreEntry]) -> Result<(), ClaudeSdkError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ClaudeSdkError::new(format!("failed to create transcript dir: {e}")))?;
    }
    let mut out = String::new();
    for entry in entries {
        out.push_str(&serde_json::to_string(entry).map_err(|e| {
            ClaudeSdkError::new(format!("failed to serialize transcript entry: {e}"))
        })?);
        out.push('\n');
    }
    tokio::fs::write(path, out)
        .await
        .map_err(|e| ClaudeSdkError::new(format!("failed to write transcript: {e}")))?;
    set_private_permissions(path);
    Ok(())
}

async fn copy_auth_files(tmp_base: &Path, opt_env: &BTreeMap<String, String>) {
    let caller_config_dir = opt_env
        .get("CLAUDE_CONFIG_DIR")
        .cloned()
        .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok());
    let source_config_dir = caller_config_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude"));

    let creds_path = source_config_dir.join(".credentials.json");
    if let Ok(creds_json) = tokio::fs::read_to_string(creds_path).await {
        let dst = tmp_base.join(".credentials.json");
        let redacted = redact_refresh_token(&creds_json);
        if tokio::fs::write(&dst, redacted).await.is_ok() {
            set_private_permissions(&dst);
        }
    }

    let claude_json_src = caller_config_dir
        .as_ref()
        .map(|dir| PathBuf::from(dir).join(".claude.json"))
        .unwrap_or_else(|| home_dir().join(".claude.json"));
    let _ = tokio::fs::copy(claude_json_src, tmp_base.join(".claude.json")).await;
}

fn redact_refresh_token(creds_json: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(creds_json) else {
        return creds_json.to_string();
    };
    if let Some(oauth) = value
        .as_object_mut()
        .and_then(|m| m.get_mut("claudeAiOauth"))
        .and_then(Value::as_object_mut)
    {
        oauth.remove("refreshToken");
    }
    serde_json::to_string(&value).unwrap_or_else(|_| creds_json.to_string())
}

async fn materialize_subkeys(
    store: &dyn SessionStore,
    _tmp_base: &Path,
    project_dir: &Path,
    project_key: &str,
    session_id: &str,
    timeout: Duration,
) -> Result<(), ClaudeSdkError> {
    let subkeys = match timeout_store(
        store.list_subkeys(SessionListSubkeysKey {
            project_key: project_key.into(),
            session_id: session_id.into(),
        }),
        timeout,
        &format!("SessionStore.list_subkeys() for session {session_id}"),
    )
    .await
    {
        Ok(subkeys) => subkeys,
        Err(e) if e.message.contains("not implemented") => return Ok(()),
        Err(e) => return Err(e),
    };

    let session_dir = project_dir.join(session_id);
    for subpath in subkeys {
        if !is_safe_subpath(&subpath) {
            warn!("[SessionStore] skipping unsafe subpath from list_subkeys: {subpath}");
            continue;
        }
        let key = SessionKey {
            project_key: project_key.into(),
            session_id: session_id.into(),
            subpath: Some(subpath.clone()),
        };
        let Some(entries) = timeout_store(
            store.load(key),
            timeout,
            &format!("SessionStore.load() for session {session_id} subpath {subpath}"),
        )
        .await?
        else {
            continue;
        };

        let mut metadata: Vec<Map<String, Value>> = Vec::new();
        let mut transcript = Vec::new();
        for entry in entries {
            if entry.get("type").and_then(Value::as_str) == Some("agent_metadata") {
                if let Some(obj) = entry.as_object() {
                    metadata.push(obj.clone());
                }
            } else {
                transcript.push(entry);
            }
        }

        let sub_file = session_dir.join(format!("{subpath}.jsonl"));
        if !transcript.is_empty() {
            write_jsonl(&sub_file, &transcript).await?;
        }
        if let Some(mut meta) = metadata.pop() {
            meta.remove("type");
            let meta_path = session_dir.join(format!("{subpath}.meta.json"));
            if let Some(parent) = meta_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ClaudeSdkError::new(format!("failed to create metadata dir: {e}"))
                })?;
            }
            tokio::fs::write(&meta_path, Value::Object(meta).to_string())
                .await
                .map_err(|e| ClaudeSdkError::new(format!("failed to write metadata: {e}")))?;
            set_private_permissions(&meta_path);
        }
    }
    Ok(())
}

fn is_safe_subpath(subpath: &str) -> bool {
    if subpath.is_empty()
        || subpath.starts_with('/')
        || subpath.starts_with('\\')
        || subpath.contains('\0')
        || subpath.contains(':')
    {
        return false;
    }
    !subpath
        .split(['/', '\\'])
        .any(|part| part.is_empty() || part == "." || part == "..")
}

async fn remove_dir_all_retry(path: &Path) {
    for _ in 0..4 {
        if tokio::fs::remove_dir_all(path).await.is_ok() || !path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = tokio::fs::remove_dir_all(path).await;
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn set_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{InMemorySessionStore, SessionStore};
    use serde_json::json;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn explicit_resume_materializes_main_transcript() {
        let store = Arc::new(InMemorySessionStore::new());
        let cwd = temp_path("claude-sdk-cwd");
        tokio::fs::create_dir_all(&cwd).await.unwrap();
        let project_key = project_key_for_directory(&cwd);
        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        store
            .append(
                SessionKey {
                    project_key: project_key.clone(),
                    session_id: session_id.into(),
                    subpath: None,
                },
                vec![json!({"type": "user", "message": {"content": "hello"}})],
            )
            .await
            .unwrap();

        let opts = ClaudeAgentOptions {
            cwd: Some(cwd.clone()),
            resume: Some(session_id.into()),
            session_store: Some(store),
            ..Default::default()
        };

        let materialized = materialize_resume_session(&opts)
            .await
            .unwrap()
            .expect("materialized");
        let transcript = materialized
            .config_dir
            .join("projects")
            .join(project_key)
            .join(format!("{session_id}.jsonl"));
        let content = tokio::fs::read_to_string(&transcript).await.unwrap();
        assert!(content.contains("\"hello\""));
        assert_eq!(materialized.resume_session_id, session_id);

        materialized.cleanup().await;
        let _ = tokio::fs::remove_dir_all(cwd).await;
        assert!(!materialized.config_dir.exists());
    }

    #[tokio::test]
    async fn continue_chooses_latest_non_sidechain_session() {
        let store = Arc::new(InMemorySessionStore::new());
        let cwd = temp_path("claude-sdk-cwd");
        tokio::fs::create_dir_all(&cwd).await.unwrap();
        let project_key = project_key_for_directory(&cwd);
        let sidechain_id = "550e8400-e29b-41d4-a716-446655440000";
        let main_id = "660e8400-e29b-41d4-a716-446655440000";

        store
            .append(
                SessionKey {
                    project_key: project_key.clone(),
                    session_id: sidechain_id.into(),
                    subpath: None,
                },
                vec![json!({"type": "user", "isSidechain": true})],
            )
            .await
            .unwrap();
        store
            .append(
                SessionKey {
                    project_key,
                    session_id: main_id.into(),
                    subpath: None,
                },
                vec![json!({"type": "user", "message": {"content": "main"}})],
            )
            .await
            .unwrap();

        let opts = ClaudeAgentOptions {
            cwd: Some(cwd),
            continue_conversation: true,
            session_store: Some(store),
            ..Default::default()
        };
        let materialized = materialize_resume_session(&opts)
            .await
            .unwrap()
            .expect("materialized");
        assert_eq!(materialized.resume_session_id, main_id);
        materialized.cleanup().await;
    }

    #[test]
    fn safe_subpath_rejects_traversal() {
        assert!(is_safe_subpath("subagents/agent-1"));
        assert!(!is_safe_subpath("../secret"));
        assert!(!is_safe_subpath("/absolute"));
        assert!(!is_safe_subpath("C:secret"));
        assert!(!is_safe_subpath("subagents//agent"));
    }
}
