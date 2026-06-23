//! Session persistence: SessionStore trait, types, and reference backends.
//!
//! Mirrors the Python `claude_agent_sdk._internal.session_store` and
//! `session_resume` modules. Enables cloud-backed session resume and
//! transcript mirroring for deployments that need cross-device sync,
//! backup, or centralized audit.

pub mod batcher;
pub mod history;
pub mod import;
pub mod materialize;
pub mod memory;
pub mod mutations;
pub mod paths;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

pub use batcher::TranscriptMirrorBatcher;
pub use history::{
    get_session_info, get_session_info_from_store, get_session_messages,
    get_session_messages_from_store, get_subagent_messages, get_subagent_messages_from_store,
    list_sessions, list_sessions_from_store, list_subagents, list_subagents_from_store,
    SDKSessionInfo, SessionMessage,
};
pub use import::import_session_to_store;
pub use materialize::{
    apply_materialized_options, build_mirror_batcher, materialize_resume_session,
    MaterializedResume,
};
pub use memory::InMemorySessionStore;
pub use mutations::{
    delete_session, delete_session_via_store, fork_session, fork_session_via_store, rename_session,
    rename_session_via_store, tag_session, tag_session_via_store, ForkSessionResult,
};

/// Identifies a session transcript (or subagent transcript) in a store.
///
/// Main transcripts omit `subpath`; subagent transcripts include one like
/// `"subagents/agent-{id}"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub project_key: String,
    pub session_id: String,
    pub subpath: Option<String>,
}

/// Key argument to `list_subkeys` (no subpath).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionListSubkeysKey {
    pub project_key: String,
    pub session_id: String,
}

/// One JSONL transcript line as observed by a store adapter. Opaque —
/// adapters treat entries as pass-through blobs.
pub type SessionStoreEntry = Value;

/// Entry returned by `list_sessions`.
#[derive(Debug, Clone)]
pub struct SessionStoreListEntry {
    pub session_id: String,
    /// Last-modified time in Unix epoch milliseconds.
    pub mtime: i64,
}

/// Incrementally-maintained session summary.
#[derive(Debug, Clone)]
pub struct SessionSummaryEntry {
    pub session_id: String,
    pub mtime: i64,
    pub data: Value,
}

/// Controls when transcript-mirror entries are flushed to a SessionStore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStoreFlushMode {
    /// Buffer and flush once per turn (on the `result` message) or when the
    /// pending buffer exceeds thresholds. Default.
    Batched,
    /// Trigger a background flush after every `transcript_mirror` frame.
    Eager,
}

/// Adapter for mirroring session transcripts to external storage.
///
/// The subprocess still writes to local disk; the adapter receives a
/// secondary copy. Only `append` and `load` are required; the rest are
/// optional and raise `NotImplemented` by default.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Mirror a batch of transcript entries. Called after the subprocess's
    /// local write succeeds — durability is already guaranteed locally.
    async fn append(
        &self,
        key: SessionKey,
        entries: Vec<SessionStoreEntry>,
    ) -> Result<(), StoreError>;

    /// Load a full session for resume. Returns `None` for a key that was
    /// never written.
    async fn load(&self, key: SessionKey) -> Result<Option<Vec<SessionStoreEntry>>, StoreError>;

    /// List sessions for a project_key. Returns IDs + mtimes.
    /// Default: `NotImplemented`.
    async fn list_sessions(
        &self,
        _project_key: &str,
    ) -> Result<Vec<SessionStoreListEntry>, StoreError> {
        Err(StoreError::NotImplemented)
    }

    /// Return incrementally-maintained summaries for all sessions in one call.
    /// Default: `NotImplemented`.
    async fn list_session_summaries(
        &self,
        _project_key: &str,
    ) -> Result<Vec<SessionSummaryEntry>, StoreError> {
        Err(StoreError::NotImplemented)
    }

    /// Delete a session. Deleting a main-transcript key must cascade to all
    /// subkeys. Default: `NotImplemented`.
    async fn delete(&self, _key: SessionKey) -> Result<(), StoreError> {
        Err(StoreError::NotImplemented)
    }

    /// List all subpath keys under a session (e.g. subagent transcripts).
    /// Default: `NotImplemented`.
    async fn list_subkeys(&self, _key: SessionListSubkeysKey) -> Result<Vec<String>, StoreError> {
        Err(StoreError::NotImplemented)
    }
}

/// Error type for SessionStore operations.
#[derive(Debug, Clone)]
pub enum StoreError {
    /// Optional method not implemented by this adapter.
    NotImplemented,
    /// Adapter-specific failure (network, serialization, etc.).
    Adapter(String),
    /// Operation timed out.
    Timeout,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotImplemented => f.write_str("not implemented by this adapter"),
            StoreError::Adapter(msg) => write!(f, "adapter error: {msg}"),
            StoreError::Timeout => f.write_str("operation timed out"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Shared, thread-safe handle to a SessionStore.
pub type SharedStore = Arc<dyn SessionStore>;

/// Probe whether a store implements an optional method by calling it with
/// a dummy key and checking for `NotImplemented`.
pub async fn store_implements_list_subkeys(store: &dyn SessionStore) -> bool {
    store
        .list_subkeys(SessionListSubkeysKey {
            project_key: String::new(),
            session_id: String::new(),
        })
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trait_object_dispatch() {
        let store: Arc<dyn SessionStore> = Arc::new(memory::InMemorySessionStore::new());
        // Optional method returns NotImplemented by default if not overridden.
        let result = store.list_sessions("test").await;
        // InMemorySessionStore implements list_sessions, so this succeeds.
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn store_error_display() {
        assert_eq!(
            StoreError::NotImplemented.to_string(),
            "not implemented by this adapter"
        );
        assert_eq!(StoreError::Timeout.to_string(), "operation timed out");
        assert!(StoreError::Adapter("disk full".into())
            .to_string()
            .contains("disk full"));
    }

    #[tokio::test]
    async fn session_key_hash_and_eq() {
        let k1 = SessionKey {
            project_key: "proj".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        let k2 = SessionKey {
            project_key: "proj".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        let k3 = SessionKey {
            project_key: "proj".into(),
            session_id: "s1".into(),
            subpath: Some("subagents/agent-1".into()),
        };
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
