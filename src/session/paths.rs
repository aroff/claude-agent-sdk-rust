//! Path utilities: project-key derivation, session-key parsing, UUID validation.
//!
//! Mirrors `claude_agent_sdk._internal.sessions` path helpers.

use std::path::{Path, PathBuf};

use crate::session::SessionKey;

/// Maximum length for a sanitized path component before hash suffix.
const MAX_SANITIZED_LENGTH: usize = 200;

/// Validate that a string is a well-formed UUID.
pub fn validate_uuid(s: &str) -> Option<&str> {
    if s.len() == 36 {
        let bytes = s.as_bytes();
        let is_hex = |b: u8| b.is_ascii_hexdigit();
        bytes
            .iter()
            .enumerate()
            .all(|(i, &b)| {
                if i == 8 || i == 13 || i == 18 || i == 23 {
                    b == b'-'
                } else {
                    is_hex(b)
                }
            })
            .then_some(s)
    } else {
        None
    }
}

/// Sanitize a path for use as a directory name: replace all non-alphanumeric
/// characters with hyphens. Paths longer than 200 chars are truncated and
/// suffixed with a djb2 hash.
pub fn sanitize_path(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    let hash = simple_hash(name);
    let truncated = sanitized[..MAX_SANITIZED_LENGTH].to_string();
    format!("{truncated}-{hash}")
}

/// 32-bit djb2 hash to base36, matching the CLI's directory naming.
fn simple_hash(s: &str) -> String {
    let mut h: i64 = 0;
    for ch in s.chars() {
        let char = ch as i64;
        h = (h << 5).wrapping_sub(h).wrapping_add(char);
    }
    let h = (h as u32) as i64; // coerce to unsigned 32-bit
    let h = h.abs();
    if h == 0 {
        return "0".to_string();
    }
    let digits = "0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    let mut n = h;
    while n > 0 {
        out.push(digits.chars().nth((n % 36) as usize).unwrap());
        n /= 36;
    }
    out.reverse();
    out.into_iter().collect()
}

/// Derive the project key for a directory path (default: sanitized cwd).
pub fn project_key_for_directory(dir: &Path) -> String {
    sanitize_path(&dir.to_string_lossy())
}

/// Resolve the projects directory from env or default `~/.claude`.
pub fn get_projects_dir(
    env_override: Option<&std::collections::BTreeMap<String, String>>,
) -> PathBuf {
    if let Some(env) = env_override {
        if let Some(dir) = env.get("CLAUDE_CONFIG_DIR") {
            return PathBuf::from(dir).join("projects");
        }
    }
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir).join("projects");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join(".claude").join("projects")
}

/// Derive a SessionKey from an absolute transcript file path.
///
/// Main transcripts: `<projects_dir>/<project_key>/<session_id>.jsonl`
/// Subagent transcripts: `<projects_dir>/<project_key>/<session_id>/subagents/.../agent-<id>.jsonl`
///
/// Returns `None` if the path is not under projects_dir or has an
/// unrecognized shape.
pub fn file_path_to_session_key(file_path: &str, projects_dir: &str) -> Option<SessionKey> {
    let rel = pathdiff_relpath(file_path, projects_dir)?;
    let rel_path = PathBuf::from(&rel);
    if rel_path.is_absolute() || rel_path.starts_with("..") {
        return None;
    }

    let parts: Vec<String> = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    if parts.len() < 2 {
        return None;
    }

    let project_key = parts[0].clone();
    let second = &parts[1];

    // Main transcript: <project_key>/<session_id>.jsonl
    if parts.len() == 2 && second.ends_with(".jsonl") {
        let session_id = second.trim_end_matches(".jsonl").to_string();
        return Some(SessionKey {
            project_key,
            session_id,
            subpath: None,
        });
    }

    // Subagent transcript: <project_key>/<session_id>/subagents/.../agent-<id>.jsonl
    if parts.len() >= 4 {
        let session_id = second.clone();
        let mut subpath_parts = parts[2..].to_vec();
        if let Some(last) = subpath_parts.last_mut() {
            if last.ends_with(".jsonl") {
                *last = last.trim_end_matches(".jsonl").to_string();
            }
        }
        return Some(SessionKey {
            project_key,
            session_id,
            subpath: Some(subpath_parts.join("/")),
        });
    }

    None
}

/// Compute a relative path from `to` to `from` without requiring the
/// `pathdiff` crate. Returns `None` if the paths are on different roots
/// (Windows drives) or `to` is not under `from`.
fn pathdiff_relpath(target: &str, base: &str) -> Option<String> {
    let target_path = Path::new(target);
    let base_path = Path::new(base);

    let target_canon = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());
    let base_canon = base_path
        .canonicalize()
        .unwrap_or_else(|_| base_path.to_path_buf());

    // If base is a prefix of target, return the suffix.
    let target_components: Vec<_> = target_canon.components().collect();
    let base_components: Vec<_> = base_canon.components().collect();

    if target_components.len() < base_components.len() {
        return None;
    }

    for (t, b) in target_components.iter().zip(base_components.iter()) {
        if t != b {
            return None;
        }
    }

    let suffix: Vec<_> = target_components[base_components.len()..]
        .iter()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(suffix.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_uuid_valid() {
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_some());
    }

    #[test]
    fn validate_uuid_invalid() {
        assert!(validate_uuid("not-a-uuid").is_none());
        assert!(validate_uuid("550e8400-e29b-41d4-a716").is_none()); // too short
        assert!(validate_uuid("550e8400xw29b-41d4-a716-446655440000").is_none());
        assert!(validate_uuid("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz").is_none());
    }

    #[test]
    fn sanitize_replaces_non_alphanumeric() {
        assert_eq!(sanitize_path("/home/user/project"), "-home-user-project");
        assert_eq!(sanitize_path("simple"), "simple");
    }

    #[test]
    fn sanitize_truncates_long_paths() {
        let long = "a".repeat(300);
        let sanitized = sanitize_path(&long);
        assert!(sanitized.len() > MAX_SANITIZED_LENGTH);
        assert!(sanitized.contains('-'));
    }

    #[test]
    fn project_key_for_directory_matches_cli() {
        let key = project_key_for_directory(Path::new("/home/user/my-project"));
        assert_eq!(key, "-home-user-my-project");
    }

    #[test]
    fn file_path_to_session_key_main_transcript() {
        let key = file_path_to_session_key(
            "/home/user/.claude/projects/-home-user-proj/abc123.jsonl",
            "/home/user/.claude/projects",
        )
        .unwrap();
        assert_eq!(key.project_key, "-home-user-proj");
        assert_eq!(key.session_id, "abc123");
        assert!(key.subpath.is_none());
    }

    #[test]
    fn file_path_to_session_key_subagent_transcript() {
        let key = file_path_to_session_key(
            "/home/user/.claude/projects/-home-user-proj/abc123/subagents/agent-1.jsonl",
            "/home/user/.claude/projects",
        )
        .unwrap();
        assert_eq!(key.project_key, "-home-user-proj");
        assert_eq!(key.session_id, "abc123");
        assert_eq!(key.subpath.as_deref(), Some("subagents/agent-1"));
    }

    #[test]
    fn file_path_to_session_key_rejects_outside_projects_dir() {
        let result = file_path_to_session_key(
            "/home/user/.claude/sessions/abc.jsonl",
            "/home/user/.claude/projects",
        );
        assert!(result.is_none());
    }

    #[test]
    fn file_path_to_session_key_rejects_too_short() {
        let result = file_path_to_session_key(
            "/home/user/.claude/projects/abc.jsonl",
            "/home/user/.claude/projects",
        );
        // parts = ["abc.jsonl"], len=1 < 2 → None
        assert!(result.is_none());
    }

    #[test]
    fn file_path_to_session_key_rejects_non_jsonl() {
        let result = file_path_to_session_key(
            "/home/user/.claude/projects/-proj-/abc123.txt",
            "/home/user/.claude/projects",
        );
        assert!(result.is_none());
    }
}
