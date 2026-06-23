//! Import local Claude JSONL transcripts into a SessionStore.

use std::path::Path;

use serde_json::Value;

use crate::session::paths::file_path_to_session_key;
use crate::session::{SessionStore, StoreError};

pub async fn import_session_to_store(
    store: &dyn SessionStore,
    projects_dir: &Path,
) -> Result<usize, StoreError> {
    let mut imported = 0;
    let files = collect_jsonl_files(projects_dir);
    let projects_dir_s = projects_dir.to_string_lossy().to_string();
    for file in files {
        let Some(key) = file_path_to_session_key(&file.to_string_lossy(), &projects_dir_s) else {
            continue;
        };
        let entries = std::fs::read_to_string(&file)
            .map_err(|e| StoreError::Adapter(format!("failed to read {}: {e}", file.display())))?
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        imported += entries.len();
        store.append(key, entries).await?;
    }
    Ok(imported)
}

fn collect_jsonl_files(base: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
    walk(base, &mut out);
    out
}
