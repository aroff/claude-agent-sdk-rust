# claude-agent-sdk-rust

Rust port of the Claude Agent SDK with 1:1 Claude message compatibility.

## Workflow

```bash
# Lint
cargo clippy --all-targets -- -D warnings

# Typecheck / compile
cargo build --all-targets

# Run all tests
cargo test

# Run a specific test binary / test
cargo test --test message_parser
cargo test --test types
```

## Codebase Structure

- `src/lib.rs` - crate root + public re-exports
- `src/error.rs` - error types (ClaudeSdkError, MessageParseError, ...)
- `src/types.rs` - message types and content blocks
- `src/message_parser.rs` - `parse_message` (1:1 port of the Python parser)
- `tests/message_parser.rs` - parser tests mirroring `tests/test_message_parser.py`
- `tests/types.rs` - type tests mirroring `tests/test_types.py`
