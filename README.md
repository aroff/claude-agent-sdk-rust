# claude-agent-sdk-rust

Rust types and parser for Claude Code CLI messages, with 1:1 wire
compatibility to the Python `claude_agent_sdk` parser.

Feed it the newline-delimited JSON that `claude --output-format stream-json`
prints, and get back strongly typed Rust structs for every message the CLI
emits — assistant turns, tool calls, results, system events, rate limits,
hook events, and more.

## Install

```toml
[dependencies]
claude-agent-sdk = { path = "../claude-agent-sdk-rust" }
serde_json = "1"
```

Requires Rust 1.70+ (2021 edition).

## Quick start

```rust
use claude_agent_sdk::{parse_message, Message, ContentBlock, UserContent};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

let mut child = Command::new("claude")
    .args(["-p", "What is 2+2?", "--output-format", "stream-json", "--verbose"])
    .stdout(Stdio::piped())
    .spawn()?;

for line in BufReader::new(child.stdout.take().unwrap()).lines() {
    let line = line?;
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
    let Ok(Some(msg)) = parse_message(&value) else { continue };

    match msg {
        Message::Assistant(a) => {
            for block in &a.content {
                if let ContentBlock::Text(t) = block {
                    println!("{}", t.text);
                }
            }
        }
        Message::Result(r) => {
            println!("done: {} ({} turns, ${:.4})",
                r.subtype, r.num_turns, r.total_cost_usd.unwrap_or(0.0));
        }
        _ => {}
    }
}
```

## What you get

`parse_message(&value) -> Result<Option<Message>, MessageParseError>`

- `Ok(Some(msg))` — a typed message.
- `Ok(None)` — unrecognized type; skip it (forward compatibility).
- `Err(e)` — malformed payload; `e.data` holds the original line.

Every `Message` is a `serde`-serializable enum, so round-tripping back to
JSON is `serde_json::to_value(&msg)`.

## Supported message types

| Variant | CLI source |
| --- | --- |
| `User` | user turns, tool results returned to the model |
| `Assistant` | model text, thinking blocks, tool calls, server tool calls |
| `System` | generic system events (e.g. `start`) |
| `TaskStarted` / `TaskProgress` / `TaskNotification` / `TaskUpdated` | background task lifecycle |
| `Result` | final per-query summary (cost, turns, usage, errors) |
| `StreamEvent` | partial streaming updates |
| `RateLimitEvent` | rate-limit status changes |
| `HookEvent` | hook lifecycle events (`hook_started`, `hook_response`) |
| `MirrorError` | transcript mirror failures |

## Content blocks

Match on `ContentBlock` to inspect assistant content:

```rust
match block {
    ContentBlock::Text(t)          => println!("{}", t.text),
    ContentBlock::Thinking(t)      => println!("(thinking) {}", t.thinking),
    ContentBlock::ToolUse(t)       => println!("{}({})", t.name, t.input),
    ContentBlock::ToolResult(t)    => println!("-> {:?}", t.content),
    ContentBlock::ServerToolUse(s) => println!("server tool: {:?}", s.name),
    ContentBlock::ServerToolResult(s) => println!("server result: {:?}", s.content),
}
```

Unknown block types are silently skipped, matching the Python parser.

## Runtime API

The crate also includes an async runtime layer:

- `query()` for one-shot prompts.
- `ClaudeSDKClient` for multi-turn conversations.
- `SubprocessCLITransport` for spawning the `claude` CLI.
- Control protocol support for initialize, interrupts, permission mode/model
  changes, hooks, SDK MCP message routing, MCP reconnect/toggle, file rewind,
  task stop, MCP status, and context usage.
- `SessionStore` support for transcript mirroring and store-backed resume
  materialization.
- Session history/mutation/import helpers.
- SDK MCP tool builders for in-process Rust tools.

The Rust port is still not full Python SDK parity. Remaining gaps are mainly
Python async-iterable input ergonomics and Python-specific type introspection
for SDK MCP schemas. See [`docs/architecture.md`](docs/architecture.md) for
the current scope.

## License

MIT
