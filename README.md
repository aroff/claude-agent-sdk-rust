# Claude Agent SDK for Rust

Rust SDK for the Claude Code agent runtime. Provides typed message parsing,
a one-shot `query()` function, and a multi-turn `ClaudeSDKClient` — all built
on the Claude Code CLI's `stream-json` protocol.

## Installation

Add to `Cargo.toml`:

```toml
[dependencies]
claude-agent-sdk = { git = "https://github.com/aroff/claude-agent-sdk-rust" }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

**Prerequisites:**

- Rust 1.70+ (2021 edition)
- Claude Code CLI:
  ```bash
  curl -fsSL https://claude.ai/install.sh | bash
  ```
- `ANTHROPIC_API_KEY` set in your environment

## Quick Start

```rust
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

#[tokio::main]
async fn main() {
    let mut handle = query(
        "What is 2 + 2? Reply with just the number.",
        ClaudeAgentOptions { max_turns: Some(1), ..Default::default() },
    )
    .await
    .expect("query failed");

    while let Some(msg) = handle.next_message().await.unwrap() {
        if let Message::Assistant(a) = &msg {
            for block in &a.content {
                if let Some(t) = block.as_text() {
                    println!("Claude: {}", t.text);
                }
            }
        }
    }
    handle.close().await.unwrap();
}
```

Run the bundled example: `cargo run --example quick_start`

## Basic Usage: `query()`

`query()` is the simplest entry point. It spawns the CLI, sends a single
prompt, and returns a `QueryHandle` that yields typed `Message` values.

```rust
use claude_agent_sdk::{query, ClaudeAgentOptions, Message, SystemPrompt};

let opts = ClaudeAgentOptions {
    system_prompt: Some(SystemPrompt::Custom(
        "You are a helpful assistant.".into(),
    )),
    max_turns: Some(1),
    ..Default::default()
};

let mut handle = query("Explain Rust in one sentence.", opts).await?;
while let Some(msg) = handle.next_message().await? {
    if let Message::Assistant(a) = &msg {
        for block in &a.content {
            if let Some(t) = block.as_text() { println!("{}", t.text); }
        }
    }
    if let Message::Result(r) = &msg {
        println!("Turns: {}  Cost: ${:.4}", r.num_turns, r.total_cost_usd.unwrap_or(0.0));
    }
}
handle.close().await?;
```

### Streaming message input

Use `query_with_messages` when you want to supply messages as a
`futures::Stream` instead of a single string — mirroring the Python SDK's
`query(prompt=async_iterable)` path:

```rust
use claude_agent_sdk::{query_with_messages, ClaudeAgentOptions};
use futures::stream;
use serde_json::json;

let messages = stream::iter(vec![json!({
    "type": "user",
    "session_id": "",
    "message": {"role": "user", "content": "What is 6 × 7?"},
    "parent_tool_use_id": null,
})]);

let mut handle = query_with_messages(messages, ClaudeAgentOptions::default()).await?;
// consume handle.next_message() as usual
```

Run the bundled example: `cargo run --example stream_input`

## `ClaudeSDKClient`

`ClaudeSDKClient` keeps the subprocess alive across multiple turns, enabling
follow-up messages, interrupts, and dynamic control.

```rust
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
client.connect(None).await?;

// Turn 1
client.query("What is the capital of France?", None).await?;
loop {
    match client.receive_message().await? {
        Some(Message::Assistant(a)) => { /* print text */ }
        Some(Message::Result(_)) | None => break,
        _ => {}
    }
}

// Turn 2 — Claude remembers the previous answer
client.query("What language do they speak there?", None).await?;
// ... read messages ...

client.disconnect().await?;
```

Run the bundled example: `cargo run --example multi_turn`

### Custom Tools (in-process SDK MCP Servers)

Register Rust async functions as MCP tools that Claude can call directly — no
separate subprocess required.

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
        let result = args["a"].as_f64().unwrap_or(0.0) + args["b"].as_f64().unwrap_or(0.0);
        json!({"content":[{"type":"text","text": result.to_string()}]})
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
client.connect(None).await?;
client.query("What is 15 + 27? Use your tools.", None).await?;
// ... read messages ...
```

Run the bundled example: `cargo run --example sdk_mcp_calculator`

## Message Types

Match on `Message` to handle each CLI output variant:

```rust
match msg {
    Message::Assistant(a) => {
        for block in &a.content {
            if let Some(t) = block.as_text()           { println!("{}", t.text) }
            if let Some(t) = block.as_thinking()        { /* thinking block */ }
            if let Some(u) = block.as_tool_use()        { /* tool call */ }
            if let Some(r) = block.as_tool_result()     { /* tool result */ }
        }
    }
    Message::User(u)       => { /* user turn / tool result feed-back */ }
    Message::System(s)     => { /* system event — s.subtype, s.data */ }
    Message::Result(r)     => {
        println!("done: {} ({} turns, ${:.4})",
            r.subtype, r.num_turns, r.total_cost_usd.unwrap_or(0.0));
    }
    Message::RateLimitEvent(rl) => { /* rate-limit status */ }
    Message::HookEvent(h)       => { /* hook lifecycle */ }
    _ => {}
}
```

Full type list: `User`, `Assistant`, `System`, `TaskStarted`, `TaskProgress`,
`TaskNotification`, `TaskUpdated`, `Result`, `StreamEvent`, `RateLimitEvent`,
`HookEvent`, `MirrorError`.

## Error Handling

```rust
use claude_agent_sdk::{
    ClaudeSdkError,      // base error
    CliNotFoundError,    // Claude Code CLI not installed
    CliConnectionError,  // failed to start the subprocess
    ProcessError,        // subprocess exited non-zero
    CliJsonDecodeError,  // malformed JSON from the CLI
};

match query("Hello", ClaudeAgentOptions::default()).await {
    Ok(mut handle) => { /* ... */ }
    Err(e) if e.message.contains("not found") => eprintln!("Install the Claude CLI first"),
    Err(e) => eprintln!("Error: {e}"),
}
```

## Control Protocol

`ClaudeSDKClient` exposes the full control-protocol surface:

```rust
client.interrupt().await?;
client.set_permission_mode(PermissionMode::AcceptEdits).await?;
client.set_model(Some("claude-opus-4-8")).await?;
client.reconnect_mcp_server("my-server").await?;
client.toggle_mcp_server("my-server", false).await?;
client.rewind_files("user-message-uuid").await?;
client.stop_task("task-id").await?;

let status = client.get_mcp_status().await?;
let usage  = client.get_context_usage().await?;
let info   = client.get_server_info().await?;
```

## Low-level: parsing stream-json

If you just want to parse the raw output of `claude --output-format stream-json`:

```rust
use claude_agent_sdk::{parse_message, Message};
use serde_json::Value;

// `line` is one newline from claude stdout
let v: Value = serde_json::from_str(&line)?;
match parse_message(&v) {
    Ok(Some(msg)) => { /* handle typed message */ }
    Ok(None)      => { /* unknown type — skip (forward compat) */ }
    Err(e)        => eprintln!("parse error: {e}"),
}
```

## Examples

| Example | Description |
|---|---|
| `cargo run --example quick_start` | One-shot `query()` with basic options |
| `cargo run --example multi_turn` | Multi-turn conversation with `ClaudeSDKClient` |
| `cargo run --example sdk_mcp_calculator` | In-process MCP calculator tools |
| `cargo run --example stream_input` | Streaming message input with `query_with_messages` |

## Development

```bash
# Lint
cargo clippy --all-targets -- -D warnings

# Tests
cargo test

# Live e2e tests (requires Claude CLI + API key)
cargo test --ignored
```

## License

MIT
