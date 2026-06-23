//! In-memory reference SessionStore implementation.

use std::collections::HashMap;

use async_trait::async_trait;

use super::{
    SessionKey, SessionListSubkeysKey, SessionStore, SessionStoreEntry, SessionStoreListEntry,
    SessionSummaryEntry, StoreError,
};

/// In-memory SessionStore for testing and development. Data is lost when
/// the process exits.
pub struct InMemorySessionStore {
    store: tokio::sync::Mutex<Inner>,
}

struct Inner {
    entries: HashMap<String, Vec<SessionStoreEntry>>,
    mtimes: HashMap<String, i64>,
    summaries: HashMap<(String, String), SessionSummaryEntry>,
    last_mtime: i64,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            store: tokio::sync::Mutex::new(Inner {
                entries: HashMap::new(),
                mtimes: HashMap::new(),
                summaries: HashMap::new(),
                last_mtime: 0,
            }),
        }
    }

    fn key_to_string(key: &SessionKey) -> String {
        let mut parts = vec![key.project_key.clone(), key.session_id.clone()];
        if let Some(sub) = &key.subpath {
            parts.push(sub.clone());
        }
        parts.join("/")
    }

    fn next_mtime(inner: &mut Inner) -> i64 {
        let now_ms = chrono_now_ms();
        let mtime = if now_ms <= inner.last_mtime {
            inner.last_mtime + 1
        } else {
            now_ms
        };
        inner.last_mtime = mtime;
        mtime
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn append(
        &self,
        key: SessionKey,
        entries: Vec<SessionStoreEntry>,
    ) -> Result<(), StoreError> {
        let mut inner = self.store.lock().await;
        let k = Self::key_to_string(&key);
        inner.entries.entry(k.clone()).or_default().extend(entries);
        let now_ms = Self::next_mtime(&mut inner);

        if key.subpath.is_none() {
            let sk = (key.project_key.clone(), key.session_id.clone());
            let prev = inner.summaries.get(&sk).cloned();
            let folded = fold_summary(prev, &key, &inner.entries[&k], now_ms);
            inner.summaries.insert(sk, folded);
        }
        inner.mtimes.insert(k, now_ms);
        Ok(())
    }

    async fn load(&self, key: SessionKey) -> Result<Option<Vec<SessionStoreEntry>>, StoreError> {
        let inner = self.store.lock().await;
        let k = Self::key_to_string(&key);
        Ok(inner.entries.get(&k).cloned())
    }

    async fn list_sessions(
        &self,
        project_key: &str,
    ) -> Result<Vec<SessionStoreListEntry>, StoreError> {
        let inner = self.store.lock().await;
        let prefix = format!("{project_key}/");
        let mut results = Vec::new();
        for (k, mtime) in &inner.mtimes {
            if k.starts_with(&prefix) {
                let rest = &k[prefix.len()..];
                if !rest.contains('/') {
                    results.push(SessionStoreListEntry {
                        session_id: rest.to_string(),
                        mtime: *mtime,
                    });
                }
            }
        }
        Ok(results)
    }

    async fn list_session_summaries(
        &self,
        project_key: &str,
    ) -> Result<Vec<SessionSummaryEntry>, StoreError> {
        let inner = self.store.lock().await;
        Ok(inner
            .summaries
            .iter()
            .filter(|((pk, _), _)| pk == project_key)
            .map(|(_, s)| s.clone())
            .collect())
    }

    async fn delete(&self, key: SessionKey) -> Result<(), StoreError> {
        let mut inner = self.store.lock().await;
        let k = Self::key_to_string(&key);
        inner.entries.remove(&k);
        inner.mtimes.remove(&k);

        if key.subpath.is_none() {
            inner
                .summaries
                .remove(&(key.project_key.clone(), key.session_id.clone()));
            let prefix = format!("{}/{}/", key.project_key, key.session_id);
            let subkeys: Vec<String> = inner
                .entries
                .keys()
                .filter(|sk| sk.starts_with(&prefix))
                .cloned()
                .collect();
            for sk in subkeys {
                inner.entries.remove(&sk);
                inner.mtimes.remove(&sk);
            }
        }
        Ok(())
    }

    async fn list_subkeys(&self, key: SessionListSubkeysKey) -> Result<Vec<String>, StoreError> {
        let inner = self.store.lock().await;
        let prefix = format!("{}/{}/", key.project_key, key.session_id);
        let mut result = Vec::new();
        for k in inner.entries.keys() {
            if k.starts_with(&prefix) {
                result.push(k[prefix.len()..].to_string());
            }
        }
        Ok(result)
    }
}

/// Fold new entries into an existing summary, returning the updated summary.
///
/// Minimal implementation: tracks entry count and last message timestamp.
/// Real backends may maintain richer summaries; this is sufficient for the
/// `list_session_summaries` fast-path.
fn fold_summary(
    prev: Option<SessionSummaryEntry>,
    _key: &SessionKey,
    entries: &[SessionStoreEntry],
    mtime: i64,
) -> SessionSummaryEntry {
    let session_id = _key.session_id.clone();
    let mut data = prev
        .as_ref()
        .and_then(|s| s.data.as_object().cloned())
        .unwrap_or_default();
    let count = data
        .get("entry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    data.insert(
        "entry_count".into(),
        serde_json::Value::Number((entries.len() as u64).into()),
    );

    // Track the last UUID seen (for display purposes).
    if let Some(last) = entries.last() {
        if let Some(uuid) = last.get("uuid").and_then(|v| v.as_str()) {
            data.insert(
                "last_uuid".into(),
                serde_json::Value::String(uuid.to_string()),
            );
        }
    }

    let _ = count; // future: incremental fold

    SessionSummaryEntry {
        session_id,
        mtime,
        data: serde_json::Value::Object(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(typ: &str, uuid: &str) -> SessionStoreEntry {
        json!({"type": typ, "uuid": uuid, "timestamp": "2025-01-01T00:00:00Z"})
    }

    #[tokio::test]
    async fn append_and_load_round_trip() {
        let store = InMemorySessionStore::new();
        let key = SessionKey {
            project_key: "proj".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        store
            .append(
                key.clone(),
                vec![entry("user", "u1"), entry("assistant", "a1")],
            )
            .await
            .unwrap();
        let loaded = store.load(key).await.unwrap().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["uuid"], "u1");
    }

    #[tokio::test]
    async fn load_missing_key_returns_none() {
        let store = InMemorySessionStore::new();
        let key = SessionKey {
            project_key: "proj".into(),
            session_id: "missing".into(),
            subpath: None,
        };
        assert!(store.load(key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_sessions_returns_mtimes() {
        let store = InMemorySessionStore::new();
        let k1 = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        let k2 = SessionKey {
            project_key: "p".into(),
            session_id: "s2".into(),
            subpath: None,
        };
        store
            .append(k1.clone(), vec![entry("user", "u1")])
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store
            .append(k2.clone(), vec![entry("user", "u2")])
            .await
            .unwrap();

        let sessions = store.list_sessions("p").await.unwrap();
        assert_eq!(sessions.len(), 2);
        // s2 was written later → higher mtime.
        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert!(s2.mtime > s1.mtime);
    }

    #[tokio::test]
    async fn list_sessions_excludes_subpaths() {
        let store = InMemorySessionStore::new();
        let main = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        let sub = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: Some("subagents/agent-1".into()),
        };
        store.append(main, vec![entry("user", "u1")]).await.unwrap();
        store.append(sub, vec![entry("user", "sub")]).await.unwrap();

        let sessions = store.list_sessions("p").await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s1");
    }

    #[tokio::test]
    async fn delete_cascades_to_subkeys() {
        let store = InMemorySessionStore::new();
        let main = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        let sub = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: Some("subagents/agent-1".into()),
        };
        store
            .append(main.clone(), vec![entry("user", "u1")])
            .await
            .unwrap();
        store.append(sub, vec![entry("user", "sub")]).await.unwrap();

        store.delete(main.clone()).await.unwrap();
        assert!(store.load(main).await.unwrap().is_none());

        let subkeys = store
            .list_subkeys(SessionListSubkeysKey {
                project_key: "p".into(),
                session_id: "s1".into(),
            })
            .await
            .unwrap();
        assert!(subkeys.is_empty(), "subkeys should be cascade-deleted");
    }

    #[tokio::test]
    async fn list_subkeys_returns_paths() {
        let store = InMemorySessionStore::new();
        let sub1 = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: Some("subagents/agent-1".into()),
        };
        let sub2 = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: Some("subagents/agent-2".into()),
        };
        store.append(sub1, vec![entry("user", "a1")]).await.unwrap();
        store.append(sub2, vec![entry("user", "a2")]).await.unwrap();

        let subkeys = store
            .list_subkeys(SessionListSubkeysKey {
                project_key: "p".into(),
                session_id: "s1".into(),
            })
            .await
            .unwrap();
        assert_eq!(subkeys.len(), 2);
        assert!(subkeys.contains(&"subagents/agent-1".to_string()));
        assert!(subkeys.contains(&"subagents/agent-2".to_string()));
    }

    #[tokio::test]
    async fn list_session_summaries_after_append() {
        let store = InMemorySessionStore::new();
        let key = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        store
            .append(key, vec![entry("user", "u1"), entry("assistant", "a1")])
            .await
            .unwrap();

        let summaries = store.list_session_summaries("p").await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].session_id, "s1");
        assert_eq!(summaries[0].data["entry_count"], 2);
        assert_eq!(summaries[0].data["last_uuid"], "a1");
    }

    #[tokio::test]
    async fn append_accumulates_entries() {
        let store = InMemorySessionStore::new();
        let key = SessionKey {
            project_key: "p".into(),
            session_id: "s1".into(),
            subpath: None,
        };
        store
            .append(key.clone(), vec![entry("user", "u1")])
            .await
            .unwrap();
        store
            .append(key.clone(), vec![entry("assistant", "a1")])
            .await
            .unwrap();

        let loaded = store.load(key).await.unwrap().unwrap();
        assert_eq!(loaded.len(), 2);
    }
}
