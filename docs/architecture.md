# Architecture

This crate is a Rust port of the Claude Agent SDK. It parses wire payloads
emitted by the Claude Code CLI (`--output-format stream-json`) into typed Rust
structs, and includes an async runtime layer for spawning the CLI, running the
control protocol, and mirroring/resuming sessions through `SessionStore`.

## Scope

| Layer | Status | Notes |
| --- | --- | --- |
| Message types (content blocks, messages, options enums) | ✅ Implemented | `src/types.rs` |
| `parse_message` (the parser) | ✅ Implemented | `src/message_parser.rs` |
| Error types | ✅ Implemented | `src/error.rs` |
| Transport (subprocess spawning) | ✅ Implemented | `src/transport/` |
| Control protocol | ✅ Implemented | `src/control.rs` |
| Public async API | ✅ Implemented | `src/query.rs`, `src/client.rs` |
| SessionStore mirroring and resume materialization | ✅ Implemented | `src/session/` |
| Session listing/mutations/import helpers | ✅ Implemented | `src/session/history.rs`, `mutations.rs`, `import.rs` |
| High-level SDK MCP tool builders | ✅ Implemented | `src/sdk_mcp.rs` |

## Module layout

```
src/
├── lib.rs            # Crate root; public re-exports
├── error.rs          # ClaudeSdkError, MessageParseError, CliNotFoundError, ...
├── types.rs          # Message structs, ContentBlock enum, PermissionUpdate
├── message_parser.rs # parse_message(&Value) -> Result<Option<Message>, _>
├── options.rs        # ClaudeAgentOptions and CLI argument building
├── transport/        # Subprocess transport
├── control.rs        # Control request/response routing
├── query.rs          # One-shot query API
├── client.rs         # Interactive client API
└── session/          # SessionStore, mirroring, and resume materialization
```

Tests live under `tests/`:

```
tests/
├── message_parser.rs         # Mirrors tests/test_message_parser.py (67 tests)
├── types.rs                  # Mirrors tests/test_types.py message portion (13 tests)
├── errors.rs                 # Mirrors tests/test_errors.py (5 tests)
└── real_transcript_compat.rs # Real CLI transcript round-trip (1 test, ~270k lines)
```

## Data flow

```
claude CLI  ──stream-json──>  stdout lines
                                  │
                                  ▼
                      serde_json::from_str  ──>  serde_json::Value
                                  │
                                  ▼
                        parse_message(&value)
                                  │
                    ┌─────────────┼──────────────┐
                    ▼             ▼              ▼
            Ok(Some(msg))   Ok(None)       Err(MessageParseError)
              │                                │
              ▼                                ▼
       typed Message             (forward-compat skip)   (malformed payload)
       (enum variant)                                   data attached for
                                                        diagnostics
```

### Forward compatibility

Unrecognized message *types* (the top-level `type` field) return `Ok(None)`
and must be skipped by the caller. Unrecognized content *block* types inside
a message are silently dropped (the rest of the message still parses). Both
behaviors match the Python parser and prevent newer CLI versions from
crashing older SDK consumers.

### Error attachment

Every `MessageParseError` carries the original payload in `.data` so callers
can log or replay the offending line. The dispatcher in `parse_message`
attaches it uniformly; per-type helper functions construct errors without
the data and the dispatcher fills it in.

## Type hierarchy

```
Message (enum)
├── User(UserMessage)            content: UserContent (Text | Blocks)
├── Assistant(AssistantMessage)  content: Vec<ContentBlock>
├── System(SystemMessage)        subtype + raw data
├── TaskStarted(TaskStartedMessage)        ┐
├── TaskProgress(TaskProgressMessage)      │  all system-family variants
├── TaskNotification(TaskNotificationMessage)│  expose base SystemMessageView
├── TaskUpdated(TaskUpdatedMessage)        │  via Message::as_system()
├── MirrorError(MirrorErrorMessage)        │  for backward compat
├── HookEvent(HookEventMessage)            ┘
├── Result(ResultMessage)
├── StreamEvent(StreamEvent)
└── RateLimitEvent(RateLimitEvent)

ContentBlock (enum, serde tag = "type")
├── Text(TextBlock)
├── Thinking(ThinkingBlock)
├── ToolUse(ToolUseBlock)
├── ToolResult(ToolResultBlock)
├── ServerToolUse(ServerToolUseBlock)       # advisor, web_search, ...
└── ServerToolResult(ServerToolResultBlock) # advisor_tool_result
```

## Remaining parity gaps

The Python SDK still has runtime ergonomics that are not ported:

1. Streaming input ergonomics equivalent to Python async iterables.
2. Python TypedDict/type-annotation schema inference for SDK MCP tools. Rust
   callers pass explicit JSON Schema values.
3. Full process-exit child tracking parity with Python/TypeScript.

## Compatibility verification

`tests/real_transcript_compat.rs` round-trips real `.jsonl` transcripts
written by the installed `claude` binary through `parse_message`. It
verifies every well-formed JSON line parses without error. As of the last
run it covers ~270,000 lines across ~1,700 real transcripts with a 99.99%+
parse rate (the remainder are on-disk corruption from CLI crashes, not
parser gaps).
