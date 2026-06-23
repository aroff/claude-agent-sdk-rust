//! Real-world compatibility validation.
//!
//! Round-trips actual `claude` binary transcript output (the `.jsonl` files
//! under `~/.claude/projects/...`) through [`parse_message`]. This is the
//! strongest possible check that the Rust parser is 1:1 compatible with the
//! CLI wire format: every line the real binary writes must parse without an
//! unexpected error.
//!
//! On-disk transcript entries share the same `type` discriminator as the
//! streaming messages the SDK consumes; types not modeled by the parser
//! (e.g. `summary`, titles) correctly return `Ok(None)` for forward
//! compatibility. The test fails only on `Err` — a real parse failure that
//! would crash an SDK consumer.
//!
//! Skips automatically when no transcripts are found (e.g. CI without a
//! local Claude install) so it never produces spurious failures.

use claude_agent_sdk::parse_message;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

fn claude_projects_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".claude").join("projects");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Recursively collect every SDK transcript `.jsonl` file under `root`.
///
/// Excludes non-SDK artifacts that happen to live under `.claude/projects/`:
/// - `journal.jsonl` / anything under a `workflows/` directory: these are
///   newton workflow journals with a different wire format
///   (`{"type":"result","key":...,"agentId":...}`) that is never emitted on
///   the CLI subprocess stdout stream the SDK consumes. The Python parser
///   would reject them identically.
fn collect_transcripts(root: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip newton workflow artifact directories entirely.
            if path.file_name().and_then(|n| n.to_str()) == Some("workflows") {
                continue;
            }
            collect_transcripts(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            // Skip workflow journals by name regardless of location.
            if path.file_name().and_then(|n| n.to_str()) == Some("journal.jsonl") {
                continue;
            }
            out.push(path);
        }
    }
}

fn parse_all_lines(path: &PathBuf) -> (usize, usize, usize, Vec<(usize, String, String)>) {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (0, 0, 0, Vec::new()),
    };
    let reader = BufReader::new(file);
    let mut total = 0usize;
    let mut ok = 0usize;
    let mut corrupt = 0usize; // invalid JSON on disk (CLI crash / truncated write)
    let mut failures = Vec::new(); // well-formed JSON the parser rejected

    for (idx, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        total += 1;
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                // Disk corruption: two records concatenated mid-write, or a
                // truncated final line from a CLI crash. This never appears
                // on the live subprocess stdout stream the SDK consumes, and
                // no JSON parser (Python's json.loads included) can recover
                // it. Counted separately so it never masks real parser gaps.
                corrupt += 1;
                continue;
            }
        };
        match parse_message(&value) {
            Ok(_) => ok += 1,
            Err(e) => failures.push((idx, trimmed.to_string(), e.message)),
        }
    }
    (total, ok, corrupt, failures)
}

#[test]
fn real_cli_transcripts_parse_without_unexpected_errors() {
    let Some(root) = claude_projects_dir() else {
        eprintln!("[skip] no ~/.claude/projects directory — skipping real-transcript validation");
        return;
    };
    let mut transcripts = Vec::new();
    collect_transcripts(&root, &mut transcripts);
    if transcripts.is_empty() {
        eprintln!("[skip] no .jsonl transcripts found under {root:?}");
        return;
    }

    let mut total_lines = 0usize;
    let mut total_ok = 0usize;
    let mut total_corrupt = 0usize;
    let mut all_failures: Vec<(PathBuf, usize, String, String)> = Vec::new();

    for path in &transcripts {
        let (total, ok, corrupt, mut failures) = parse_all_lines(path);
        total_lines += total;
        total_ok += ok;
        total_corrupt += corrupt;
        for (idx, line, msg) in failures.drain(..) {
            all_failures.push((path.clone(), idx, line, msg));
        }
    }

    assert!(
        total_lines > 0,
        "sanity: transcripts contained no parseable lines"
    );

    // The guarantee: every well-formed JSON line the real `claude` binary
    // wrote must parse. Disk-corruption (truncated/concatenated writes from
    // CLI crashes) is reported separately and never appears on the live
    // stdout stream, so it does not fail this assertion.
    if !all_failures.is_empty() {
        let shown = all_failures.len().min(10);
        for (path, idx, line, msg) in all_failures.iter().take(shown) {
            eprintln!(
                "FAIL {}:{idx}: {msg}\n    line: {}",
                path.display(),
                line.chars().take(200).collect::<String>()
            );
        }
        let extra = all_failures.len().saturating_sub(shown);
        if extra > 0 {
            eprintln!("  ... and {extra} more parser failures");
        }
    }

    assert!(
        all_failures.is_empty(),
        "{} well-formed line(s) failed to parse across {} transcripts \
         ({} additional disk-corruption lines skipped)",
        all_failures.len(),
        transcripts.len(),
        total_corrupt,
    );
    let pct = (total_ok as f64) / (total_lines as f64) * 100.0;
    eprintln!(
        "[ok] parsed {total_ok}/{total_lines} well-formed lines ({pct:.3}%) \
         across {} real transcripts; {total_corrupt} corrupt line(s) skipped",
        transcripts.len()
    );
}
