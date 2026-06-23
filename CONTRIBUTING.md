# Contributing

Contributions welcome. This document covers how to build, test, and extend
the crate.

## Setup

```bash
git clone <repo>
cd claude-agent-sdk-rust
cargo build
cargo test
```

Requires Rust 1.70+ (2021 edition).

## Workflow

```bash
# Typecheck / compile all targets
cargo build --all-targets

# Run the full test suite
cargo test

# Run one test binary
cargo test --test message_parser
cargo test --test types
cargo test --test errors

# Lint (must be clean before committing)
cargo clippy --all-targets -- -D warnings

# Format check
cargo fmt --check
```

### Test suite

| File | What it covers |
| --- | --- |
| `tests/message_parser.rs` | Parser behavior — 1:1 mirror of `tests/test_message_parser.py` plus extras |
| `tests/types.rs` | Type construction and serde round-trips |
| `tests/errors.rs` | Error type behavior — mirror of `tests/test_errors.py` |
| `tests/real_transcript_compat.rs` | Round-trips real `claude` CLI transcripts from `~/.claude/projects/` |
| `src/*` unit tests | Inline tests for enums and error helpers |

The real-transcript test auto-skips when no `~/.claude/projects` directory
exists, so it runs in CI without a local Claude install. Locally it covers
~270,000 lines of real output.

## Project conventions

- **No comments unless requested.** Match the existing style — code is
  self-documenting; doc-comments on public items only.
- **Forward compatibility is load-bearing.** Unknown top-level message types
  return `Ok(None)`. Unknown content block types are silently dropped. Do
  not add a catch-all error path for these — it breaks newer CLI versions
  against older SDK builds.
- **Mirrors, not reinterpretations.** When porting behavior from the Python
  SDK, preserve field names, error messages, and null/absent distinctions
  exactly. The Python `parse_message` is the source of truth; deviations
  need a justification.
- **`serde` round-trip.** Every public type derives `Serialize` + `Deserialize`
  with the wire-format field names. Verify with a round-trip test before
  landing new types.

## Adding a new message type

1. Add the struct to `src/types.rs` with `Serialize`/`Deserialize` and
   `#[serde(default, skip_serializing_if = "Option::is_none")]` on optional
   fields.
2. Add a variant to the `Message` enum (and update `Message::as_system` /
   `is_system` if it is system-family).
3. Add a branch to `parse_message` in `src/message_parser.rs` with the same
   required-field checks and the same "Missing required field in <type>
   message: <key>" error format.
4. Re-export from `src/lib.rs`.
5. Add tests in `tests/message_parser.rs` mirroring the equivalent Python
   case (if one exists), plus an optional-fields-absent case.

## Compatibility verification

Before claiming 1:1 compatibility, run the real-transcript test locally:

```bash
cargo test --test real_transcript_compat -- --nocapture
```

The test reports the parse rate across all transcripts. A drop below
99.99% on well-formed lines signals a regression.

## Commit style

Conventional Commits:

```
feat(parser): support new advisor_redacted_result block
fix(types): preserve is_error on tool results from subagents
test(message_parser): add optional-fields-absent case for stream events
docs: clarify transport scope in architecture.md
```

## License

By contributing you agree your changes are licensed MIT.
