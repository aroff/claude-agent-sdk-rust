---
name: claude-agent-sdk-rust
description: Parse Claude Code CLI stream-json output into typed Rust message structs with 1:1 wire compatibility to the Python claude_agent_sdk parser. Use when consuming claude --output-format stream-json lines, inspecting assistant/user/system/result/rate-limit/hook messages, or matching on content blocks (text, thinking, tool_use, tool_result, server_tool_use, advisor_tool_result).
---

# claude-agent-sdk-rust

Rust SDK for Claude Code CLI messages and runtime integration.

## When to use

- You are spawning `claude --output-format stream-json` and need to handle
  its newline-delimited JSON output in Rust.
- You need typed structs for assistant text, tool calls, tool results,
  thinking blocks, server tool calls (advisor), system events, task
  lifecycle, rate limits, hook events, or final results.
- You are porting Python `claude_agent_sdk` message handling to Rust.
- You need the Rust async runtime layer: `query()`, `ClaudeSDKClient`,
  `SubprocessCLITransport`, control protocol requests, or `SessionStore`
  transcript mirroring and resume materialization.
- You need session history helpers (`list_sessions`, `get_session_messages`,
  rename/tag/delete/fork/import) or in-process SDK MCP tool builders.

## When NOT to use

- You need exact Python internals such as TypedDict introspection. Rust SDK
  MCP tools accept explicit JSON Schema values instead.
- You need a non-Rust SDK.

## Install

```toml
[dependencies]
claude-agent-sdk = { path = "../claude-agent-sdk-rust" }
serde_json = "1"
```

## Core pattern

### Parse raw stream-json lines

```rust
use claude_agent_sdk::{parse_message, Message, ContentBlock};

// `line` is one newline from `claude --output-format stream-json` stdout.
let value: serde_json::Value = serde_json::from_str(&line)?;
match parse_message(&value) {
    Ok(Some(Message::Assistant(a))) => {
        for block in &a.content {
            if let ContentBlock::Text(t) = block {
                println!("{}", t.text);
            }
        }
    }
    Ok(Some(Message::Result(r))) => {
        println!("done: {} (${:.4})", r.subtype, r.total_cost_usd.unwrap_or(0.0));
    }
    Ok(Some(other)) => { /* User, System, Task*, RateLimitEvent, ... */ }
    Ok(None) => {}                       // unknown type — skip (forward compat)
    Err(e) => eprintln!("parse error: {e} (data: {:?})", e.data),
}
```

### Run a one-shot query

```rust
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

let mut handle = query(
    "What is 2+2? Reply with just the number.",
    ClaudeAgentOptions::default(),
).await?;

while let Some(message) = handle.next_message().await? {
    if let Message::Assistant(a) = message {
        for block in a.content {
            if let Some(text) = block.as_text() {
                println!("{}", text.text);
            }
        }
    }
}
handle.close().await?;
```

### Use the interactive client

```rust
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient};

let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
client.connect(None).await?;
client.query("Start a short conversation.", None).await?;
client.set_permission_mode(claude_agent_sdk::PermissionMode::Plan).await?;
client.disconnect().await?;
```

## API reference

### Entry point

`parse_message(&serde_json::Value) -> Result<Option<Message>, MessageParseError>`

- `Ok(Some(msg))` — recognized message type.
- `Ok(None)` — unrecognized top-level type; skip it.
- `Err(MessageParseError)` — malformed payload. `.data` holds the original
  line; `.message` carries the same "Missing required field in <type>
  message: <key>" format as the Python SDK.

### Message enum

Match on `Message`:

| Variant | Key fields |
| --- | --- |
| `User(UserMessage)` | `content: UserContent` (Text or Blocks), `uuid`, `parent_tool_use_id`, `tool_use_result` |
| `Assistant(AssistantMessage)` | `content: Vec<ContentBlock>`, `model`, `usage`, `stop_reason`, `error`, `message_id`, `session_id`, `uuid` |
| `System(SystemMessage)` | `subtype`, `data` (raw) |
| `TaskStarted` / `TaskProgress` / `TaskNotification` / `TaskUpdated` | task lifecycle; all expose `.as_system()` for backward-compat |
| `Result(ResultMessage)` | `subtype`, `duration_ms`, `is_error`, `num_turns`, `session_id`, `total_cost_usd`, `usage`, `model_usage`, `errors`, `deferred_tool_use` |
| `StreamEvent(StreamEvent)` | `event`, `uuid`, `session_id` |
| `RateLimitEvent(RateLimitEvent)` | `rate_limit_info` (status, resets_at, rate_limit_type, utilization, overage_*, raw) |
| `HookEvent(HookEventMessage)` | `subtype` (hook_started/hook_response), `hook_event_name`, `session_id`, `uuid` |
| `MirrorError(MirrorErrorMessage)` | `key`, `error` |

`Message::is_system()` is true for every system-family variant.
`Message::as_system()` returns a `SystemMessageView { subtype, data }` for
any system-family message.

### Content blocks

```rust
match block {
    ContentBlock::Text(t)             => &t.text,
    ContentBlock::Thinking(t)         => &t.thinking,    // + t.signature
    ContentBlock::ToolUse(t)          => t.name, t.id, t.input,
    ContentBlock::ToolResult(t)       => t.tool_use_id, t.content, t.is_error,
    ContentBlock::ServerToolUse(s)    => s.name (ServerToolName), s.input,
    ContentBlock::ServerToolResult(s) => s.tool_use_id, s.content,
}
```

Convenience accessors: `block.as_text()`, `as_thinking()`, `as_tool_use()`,
`as_tool_result()`, `as_server_tool_use()`, `as_server_tool_result()`.

Unknown block types are silently skipped — do not expect a catch-all
variant.

### Terminal task status

```rust
use claude_agent_sdk::terminal_task_statuses;
if terminal_task_statuses().contains(status) { /* done */ }
// = {"completed", "failed", "stopped", "killed"}
```

### Permission updates

```rust
use claude_agent_sdk::{PermissionUpdate, PermissionMode};
let update = PermissionUpdate::from_dict(&map)?;
let wire = update.to_dict();  // inverse round-trip
```

### Errors

`ClaudeSdkError` (base), `CliNotFoundError`, `CliConnectionError`,
`ProcessError` (exit_code, stderr), `CliJsonDecodeError`, `MessageParseError`
(data). All implement `std::error::Error`.

## Runtime and SessionStore

Use `ClaudeAgentOptions.session_store` to mirror transcript entries to a
store. When `resume` or `continue_conversation` is set, the SDK can
materialize matching store entries into a temporary Claude config directory
before spawning the CLI. Transcript mirror frames are flushed before `result`
messages are yielded, and final flush runs during query/client close.

Read `references/runtime-session-store.md` for a focused sample and parity
notes.

## Session History and Mutations

Use the session APIs to inspect or modify transcript history:

- `list_sessions`, `get_session_info`, `get_session_messages`
- `list_subagents`, `get_subagent_messages`
- store-backed variants ending in `_from_store`
- `rename_session`, `tag_session`, `delete_session`, `fork_session`
- store-backed mutation variants ending in `_via_store`
- `import_session_to_store`

## SDK MCP Tool Builders

Use `tool(...)` and `create_sdk_mcp_server(...)` to expose in-process Rust
handlers through the existing control-protocol MCP bridge. Rust callers pass
explicit JSON Schema values instead of Python type annotations.

## Scope

Implemented: message types, content blocks, `parse_message`, error types,
serde round-trips, subprocess transport, control protocol, `query()`,
`ClaudeSDKClient`, `SessionStore` mirroring/resume materialization, session
history/mutation/import helpers, and SDK MCP tool builders.

Remaining gap: full Python async-iterable input ergonomics. See
`docs/architecture.md`.

## Verification

Run normal checks:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run the skill validation helper from the repository root:

```bash
bash skills/skill/scripts/run-validation.sh
```

It runs formatting, clippy, tests, and ignored live e2e tests when a usable
`claude` binary/authentication is present.

## Pitfalls

- **Do not unwrap `parse_message`.** Unknown types legitimately return
  `Ok(None)` — handle that path or newer CLI output will panic your code.
- **`Value`/`Map`-carrying types do not implement `Eq`.** Use `PartialEq`
  comparisons; do not put `Message` in `HashSet`/`BTreeMap` keyed by value.
- **`content` on `ToolResultBlock`/`ResultMessage.usage` etc. is `Option<Value>`.**
  `null` and absent are both `None`.
- **Forward compat is deliberate.** Adding a catch-all error for unknown
  block types breaks compatibility with newer `claude` versions — leave the
  silent-skip behavior alone.
- **Close handles.** Call `QueryHandle::close()` or
  `ClaudeSDKClient::disconnect()` so transcript mirror buffers flush and
  temporary resume directories are removed.
