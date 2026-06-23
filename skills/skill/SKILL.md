---
name: claude-agent-sdk-rust
description: Rust SDK for the Claude Code agent runtime — typed message parsing, one-shot query(), multi-turn ClaudeSDKClient, in-process SDK MCP tool builders, SessionStore mirroring, and session history helpers. Use when spawning claude --output-format stream-json and need typed Rust structs, or when building Rust agents using the Claude Code control protocol.
---

# claude-agent-sdk-rust

Rust SDK for Claude Code CLI messages and runtime integration.

## When to use

- You are spawning `claude --output-format stream-json` and need to handle
  its newline-delimited JSON output in Rust.
- You need typed structs for assistant text, tool calls, tool results,
  thinking blocks, server tool calls, system events, task lifecycle, rate
  limits, hook events, or final results.
- You need the Rust async runtime layer: `query()`, `query_with_messages()`,
  `ClaudeSDKClient`, `SubprocessCLITransport`, control protocol requests, or
  `SessionStore` transcript mirroring and resume materialization.
- You need session history helpers (`list_sessions`, `get_session_messages`,
  rename/tag/delete/fork/import) or in-process SDK MCP tool builders.

## When NOT to use

- You need exact Python internals such as TypedDict introspection. Rust SDK
  MCP tools accept explicit JSON Schema values instead.
- You need a non-Rust SDK.

## Install

```toml
[dependencies]
claude-agent-sdk = { git = "https://github.com/aroff/claude-agent-sdk-rust" }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## Core patterns

### One-shot query

```rust
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

let mut handle = query(
    "What is 2+2? Reply with just the number.",
    ClaudeAgentOptions { max_turns: Some(1), ..Default::default() },
).await?;

while let Some(msg) = handle.next_message().await? {
    if let Message::Assistant(a) = &msg {
        for block in &a.content {
            if let Some(t) = block.as_text() { println!("{}", t.text); }
        }
    }
}
handle.close().await?;
```

### Streaming message input

```rust
use claude_agent_sdk::{query_with_messages, ClaudeAgentOptions};
use futures::stream;
use serde_json::json;

let messages = stream::iter(vec![json!({
    "type": "user",
    "session_id": "",
    "message": {"role": "user", "content": "Hello"},
    "parent_tool_use_id": null,
})]);
let mut handle = query_with_messages(messages, ClaudeAgentOptions::default()).await?;
// consume handle.next_message() as usual
```

### Multi-turn conversation

```rust
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
client.connect(None).await?;

client.query("What is the capital of France?", None).await?;
loop {
    match client.receive_message().await? {
        Some(Message::Result(_)) | None => break,
        Some(msg) => { /* handle */ }
        _ => {}
    }
}
client.disconnect().await?;
```

### In-process SDK MCP tools

```rust
use claude_agent_sdk::{
    create_sdk_mcp_server, tool, ClaudeAgentOptions, ClaudeSDKClient, McpServers, QueryConfig,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

let add = tool(
    "add",
    "Add two numbers",
    json!({"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}},"required":["a","b"]}),
    |args: Value| async move {
        let r = args["a"].as_f64().unwrap_or(0.0) + args["b"].as_f64().unwrap_or(0.0);
        json!({"content":[{"type":"text","text":r.to_string()}]})
    },
);

let server = create_sdk_mcp_server("calc", "1.0.0", vec![add]);
let server_config = server.config.clone();
let qconfig = QueryConfig::default().with_sdk_mcp_server("calc", server);

let mut mcp_map = BTreeMap::new();
mcp_map.insert("calc".into(), server_config);

let opts = ClaudeAgentOptions {
    mcp_servers: McpServers::Map(mcp_map),
    allowed_tools: vec!["mcp__calc__add".into()],
    ..Default::default()
};
let mut client = ClaudeSDKClient::with_config(opts, qconfig);
```

### Parse raw stream-json lines

```rust
use claude_agent_sdk::{parse_message, Message, ContentBlock};

let value: serde_json::Value = serde_json::from_str(&line)?;
match parse_message(&value) {
    Ok(Some(Message::Assistant(a))) => {
        for block in &a.content {
            if let ContentBlock::Text(t) = block { println!("{}", t.text); }
        }
    }
    Ok(Some(Message::Result(r))) => {
        println!("done: {} (${:.4})", r.subtype, r.total_cost_usd.unwrap_or(0.0));
    }
    Ok(Some(_)) => {}   // User, System, Task*, RateLimitEvent, ...
    Ok(None)    => {}   // unknown type — skip (forward compat)
    Err(e)      => eprintln!("parse error: {e}"),
}
```

## API reference

### Entry point

`parse_message(&serde_json::Value) -> Result<Option<Message>, MessageParseError>`

- `Ok(Some(msg))` — recognized message type.
- `Ok(None)` — unrecognized top-level type; skip (forward compat).
- `Err(MessageParseError)` — malformed payload. `.data` holds the original
  line; `.message` mirrors the Python SDK's error format.

### Message enum

| Variant | Key fields |
| --- | --- |
| `User(UserMessage)` | `content: UserContent` (Text or Blocks), `uuid`, `parent_tool_use_id`, `tool_use_result` |
| `Assistant(AssistantMessage)` | `content: Vec<ContentBlock>`, `model`, `usage`, `stop_reason`, `error`, `message_id`, `session_id`, `uuid` |
| `System(SystemMessage)` | `subtype`, `data` (raw) |
| `TaskStarted` / `TaskProgress` / `TaskNotification` / `TaskUpdated` | task lifecycle; all expose `.as_system()` |
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
    ContentBlock::ToolUse(t)          => (t.name, t.id, t.input),
    ContentBlock::ToolResult(t)       => (t.tool_use_id, t.content, t.is_error),
    ContentBlock::ServerToolUse(s)    => (s.name, s.input),
    ContentBlock::ServerToolResult(s) => (s.tool_use_id, s.content),
}
```

Convenience accessors: `block.as_text()`, `as_thinking()`, `as_tool_use()`,
`as_tool_result()`, `as_server_tool_use()`, `as_server_tool_result()`.

Unknown block types are silently skipped — do not add a catch-all.

### Errors

`ClaudeSdkError` (base), `CliNotFoundError`, `CliConnectionError`,
`ProcessError` (exit_code, stderr), `CliJsonDecodeError`, `MessageParseError`
(data). All implement `std::error::Error`.

### Control protocol (ClaudeSDKClient)

```rust
client.interrupt().await?;
client.set_permission_mode(PermissionMode::AcceptEdits).await?;
client.set_model(Some("claude-opus-4-8")).await?;
client.reconnect_mcp_server("name").await?;
client.toggle_mcp_server("name", false).await?;
client.rewind_files("user-message-uuid").await?;
client.stop_task("task-id").await?;
let status = client.get_mcp_status().await?;
let usage  = client.get_context_usage().await?;
let info   = client.get_server_info().await?;
```

## Examples

| Example | Command |
|---|---|
| One-shot query | `cargo run --example quick_start` |
| Multi-turn conversation | `cargo run --example multi_turn` |
| In-process calculator tools | `cargo run --example sdk_mcp_calculator` |
| Streaming message input | `cargo run --example stream_input` |

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --examples
```

Run the skill validation helper from the repository root:

```bash
bash skills/skill/scripts/run-validation.sh
```

## Pitfalls

- **Do not unwrap `parse_message`.** Unknown types legitimately return
  `Ok(None)` — handle that path or newer CLI output will panic your code.
- **`Value`/`Map`-carrying types do not implement `Eq`.** Use `PartialEq`
  comparisons.
- **`content` on `ToolResultBlock`/`ResultMessage.usage` etc. is `Option<Value>`.**
  `null` and absent are both `None`.
- **Forward compat is deliberate.** Unknown block types are silently skipped.
- **Close handles.** Call `QueryHandle::close()` or
  `ClaudeSDKClient::disconnect()` so transcript mirror buffers flush and
  temporary resume directories are removed.
- **SDK MCP server wiring.** `create_sdk_mcp_server` returns a
  `SdkMcpServerConfig` with two parts: `config` (goes into
  `ClaudeAgentOptions.mcp_servers`) and `handler` (registered via
  `QueryConfig::with_sdk_mcp_server`). Both must be wired for tools to work.
