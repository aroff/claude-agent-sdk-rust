//! Correctness and performance coverage for the `limit`-bounded fast path in
//! `list_sessions`. See `src/session/history.rs` for the implementation.
//!
//! `list_sessions` reads `CLAUDE_CONFIG_DIR` (via `get_projects_dir`), which
//! is process-global state. All tests here serialize on `ENV_LOCK` so they
//! never run concurrently with each other inside this test binary.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use claude_agent_sdk::session::paths::project_key_for_directory;
use claude_agent_sdk::{list_sessions, SDKSessionInfo};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct ConfigDirGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    root: PathBuf,
    previous: Option<String>,
}

impl ConfigDirGuard {
    fn new(tag: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root =
            std::env::temp_dir().join(format!("claude-sdk-test-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("projects")).unwrap();
        let previous = std::env::var("CLAUDE_CONFIG_DIR").ok();
        // SAFETY / rationale: `edition = "2021"` — `set_var`/`remove_var` are
        // safe here. Access is serialized by `ENV_LOCK` above, and no other
        // test in this crate reads/writes `CLAUDE_CONFIG_DIR`.
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);
        Self {
            _lock: lock,
            root,
            previous,
        }
    }

    fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }
}

impl Drop for ConfigDirGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn user_entry_line(uuid: &str, text: &str, cwd: &str) -> String {
    serde_json::json!({
        "type": "user",
        "uuid": uuid,
        "message": {"content": text},
        "cwd": cwd,
        "gitBranch": "main",
        "timestamp": "2024-01-01T00:00:00.000Z",
    })
    .to_string()
}

/// Writes `<project_dir>/<session_id>.jsonl` with a single valid `user`
/// entry, so the session has a non-empty summary and passes
/// `entries_to_session_info`.
fn write_valid_session(project_dir: &Path, summary_text: &str) -> String {
    std::fs::create_dir_all(project_dir).unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let line = user_entry_line(
        &uuid::Uuid::new_v4().to_string(),
        summary_text,
        &project_dir.to_string_lossy(),
    );
    std::fs::write(project_dir.join(format!("{session_id}.jsonl")), line).unwrap();
    session_id
}

/// Writes a session file with `N` lines of realistic-shaped `assistant`
/// entries, padded so that a full parse (read + per-line `serde_json`
/// decode + entry walk) costs real, measurable CPU time. The *last* entry is
/// a `user` entry so the session still gets a valid summary.
fn write_large_session(project_dir: &Path, lines: usize) -> String {
    std::fs::create_dir_all(project_dir).unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let mut body = String::new();
    let padding = "x".repeat(400);
    for i in 0..lines {
        let entry = serde_json::json!({
            "type": "assistant",
            "uuid": uuid::Uuid::new_v4().to_string(),
            "message": {
                "content": [{"type": "text", "text": format!("chunk-{i}-{padding}")}]
            },
            "timestamp": "2024-01-01T00:00:00.000Z",
        });
        body.push_str(&entry.to_string());
        body.push('\n');
    }
    body.push_str(&user_entry_line(
        &uuid::Uuid::new_v4().to_string(),
        "final prompt",
        &project_dir.to_string_lossy(),
    ));
    std::fs::write(project_dir.join(format!("{session_id}.jsonl")), body).unwrap();
    session_id
}

/// A session whose first entry is a sidechain entry: `entries_to_session_info`
/// returns `None` for these, so they must never appear in output and must
/// never count against `limit`/`offset`.
fn write_sidechain_session(project_dir: &Path) {
    std::fs::create_dir_all(project_dir).unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let line = serde_json::json!({
        "type": "user",
        "uuid": uuid::Uuid::new_v4().to_string(),
        "isSidechain": true,
        "message": {"content": "hidden"},
    })
    .to_string();
    std::fs::write(project_dir.join(format!("{session_id}.jsonl")), line).unwrap();
}

/// A session with no `user`/`custom-title`/`tag`/`summary` entries at all:
/// summary ends up empty, so `entries_to_session_info` returns `None`.
fn write_empty_summary_session(project_dir: &Path) {
    std::fs::create_dir_all(project_dir).unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let line = serde_json::json!({
        "type": "progress",
        "uuid": uuid::Uuid::new_v4().to_string(),
    })
    .to_string();
    std::fs::write(project_dir.join(format!("{session_id}.jsonl")), line).unwrap();
}

/// A `.jsonl` file whose stem is not a valid UUID: must be silently ignored
/// entirely (never even considered a candidate).
fn write_non_uuid_named_file(project_dir: &Path) {
    std::fs::create_dir_all(project_dir).unwrap();
    let line = user_entry_line("u1", "should never appear", "/tmp");
    std::fs::write(project_dir.join("not-a-session-id.jsonl"), line).unwrap();
}

fn sleep_for_mtime_gap() {
    // Generous vs. typical filesystem mtime granularity so files written in
    // sequence sort distinctly by `last_modified`.
    std::thread::sleep(Duration::from_millis(15));
}

/// Reference implementation of what `list_sessions` with a `limit` is
/// supposed to compute: the exhaustive, unbounded scan, then
/// `skip(offset).take(limit)` client-side. This is the ground truth the fast
/// path must match exactly, in both content and order.
fn expected_page(directory: Option<&Path>, limit: usize, offset: usize) -> Vec<SDKSessionInfo> {
    list_sessions(directory, None, 0, false)
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect()
}

#[test]
fn lazy_limit_path_matches_full_scan_single_directory() {
    let guard = ConfigDirGuard::new("equiv-single");
    let cwd = Path::new("/tmp/claude-sdk-equiv-project");
    let project_dir = guard.projects_dir().join(project_key_for_directory(cwd));

    let mut ids = Vec::new();
    for i in 0..6 {
        ids.push(write_valid_session(&project_dir, &format!("session {i}")));
        sleep_for_mtime_gap();
    }
    write_sidechain_session(&project_dir);
    sleep_for_mtime_gap();
    write_empty_summary_session(&project_dir);
    sleep_for_mtime_gap();
    write_non_uuid_named_file(&project_dir);
    sleep_for_mtime_gap();
    // A couple more valid sessions after the noise, so it's interleaved
    // through the sort order rather than only at the edges.
    for i in 6..8 {
        ids.push(write_valid_session(&project_dir, &format!("session {i}")));
        sleep_for_mtime_gap();
    }

    assert_eq!(ids.len(), 8, "sanity: all valid sessions were written");

    for (limit, offset) in [(1, 0), (3, 0), (3, 2), (100, 0), (2, 7), (0, 0)] {
        let actual = list_sessions(Some(cwd), Some(limit).filter(|l| *l > 0), offset, false);
        let expected = if limit == 0 {
            // `limit == Some(0)` and `limit == None` are equivalent no-op
            // bounds throughout this codebase (see `page()`); assert that
            // invariant holds for the fast path too.
            expected_page(Some(cwd), usize::MAX, offset)
        } else {
            expected_page(Some(cwd), limit, offset)
        };
        assert_eq!(
            actual, expected,
            "mismatch for limit={limit}, offset={offset}"
        );
    }
}

#[test]
fn lazy_limit_path_matches_full_scan_across_all_projects() {
    let guard = ConfigDirGuard::new("equiv-multi");
    let projects_dir = guard.projects_dir();

    let mut ids = Vec::new();
    for p in 0..3 {
        let project_dir = projects_dir.join(format!("proj-{p}"));
        for i in 0..3 {
            ids.push(write_valid_session(&project_dir, &format!("p{p}-s{i}")));
            sleep_for_mtime_gap();
        }
        write_sidechain_session(&project_dir);
        sleep_for_mtime_gap();
    }
    assert_eq!(ids.len(), 9);

    for (limit, offset) in [(2, 0), (4, 3), (20, 0), (1, 8)] {
        let actual = list_sessions(None, Some(limit), offset, false);
        let expected = expected_page(None, limit, offset);
        assert_eq!(
            actual, expected,
            "mismatch for limit={limit}, offset={offset}"
        );
    }
}

#[test]
fn lazy_limit_path_handles_limit_exceeding_available_sessions() {
    let guard = ConfigDirGuard::new("equiv-overrun");
    let cwd = Path::new("/tmp/claude-sdk-equiv-overrun");
    let project_dir = guard.projects_dir().join(project_key_for_directory(cwd));
    for i in 0..3 {
        write_valid_session(&project_dir, &format!("session {i}"));
        sleep_for_mtime_gap();
    }

    let actual = list_sessions(Some(cwd), Some(50), 1, false);
    let expected = expected_page(Some(cwd), 50, 1);
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 2);
}

/// Root-cause / regression coverage: a bounded `limit` must bound the number
/// of full transcript parses, not just the size of the final page. Seeds a
/// store where a handful of large, slow-to-parse sessions are the *oldest*
/// (by filesystem mtime) and a couple of tiny sessions are the *newest*.
/// Asking for the 2 most recent sessions must never pay the cost of parsing
/// the large ones.
///
/// This bound was calibrated against the pre-fix implementation, where it
/// reliably failed (full scan of the large files took several hundred ms);
/// against the fixed implementation it reliably passes in single-digit ms.
#[test]
fn limit_bounds_the_number_of_full_parses() {
    let guard = ConfigDirGuard::new("perf");
    let cwd = Path::new("/tmp/claude-sdk-perf-project");
    let project_dir = guard.projects_dir().join(project_key_for_directory(cwd));

    // Oldest: a batch of large sessions. If `limit` fails to bound work,
    // these get fully read + parsed line-by-line.
    for _ in 0..40 {
        write_large_session(&project_dir, 2_000);
    }
    sleep_for_mtime_gap();
    sleep_for_mtime_gap();

    // Newest: the only sessions that should ever be touched for limit=2.
    write_valid_session(&project_dir, "most recent A");
    sleep_for_mtime_gap();
    write_valid_session(&project_dir, "most recent B");

    let start = Instant::now();
    let result = list_sessions(Some(cwd), Some(2), 0, false);
    let elapsed = start.elapsed();

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].summary, "most recent B");
    assert_eq!(result[1].summary, "most recent A");

    assert!(
        elapsed < Duration::from_millis(150),
        "list_sessions(limit=2) took {elapsed:?}, expected well under 150ms; \
         this indicates the large, older sessions were fully parsed even \
         though only the 2 most recent sessions were requested"
    );
}
