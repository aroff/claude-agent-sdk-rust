# Runtime and SessionStore Sample

Use this when working on the Rust runtime layer rather than only the message
parser.

## One-shot query

```rust
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

let mut handle = query(
    "What is 2+2? Reply with just the number.",
    ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    },
)
.await?;

while let Some(message) = handle.next_message().await? {
    match message {
        Message::Assistant(a) => {
            for block in &a.content {
                if let Some(text) = block.as_text() {
                    println!("{}", text.text);
                }
            }
        }
        Message::Result(_) => break,
        _ => {}
    }
}

handle.close().await?;
```

## Interactive client

```rust
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

let mut client = ClaudeSDKClient::new(ClaudeAgentOptions {
    max_turns: Some(1),
    ..Default::default()
});

client.connect(None).await?;
client.query("Say hello in one short sentence.", None).await?;

while let Some(message) = client.receive_message().await? {
    if matches!(message, Message::Result(_)) {
        break;
    }
}

client.disconnect().await?;
```

## SessionStore mirroring

```rust
use std::sync::Arc;

use claude_agent_sdk::{
    query, ClaudeAgentOptions, InMemorySessionStore, Message, SessionStoreFlushMode,
};

let store = Arc::new(InMemorySessionStore::new());
let mut handle = query(
    "Create a tiny response.",
    ClaudeAgentOptions {
        session_store: Some(store.clone()),
        session_store_flush: SessionStoreFlushMode::Batched,
        max_turns: Some(1),
        ..Default::default()
    },
)
.await?;

while let Some(message) = handle.next_message().await? {
    if matches!(message, Message::Result(_)) {
        break;
    }
}

handle.close().await?;
```

The CLI only emits transcript mirror frames when `--session-mirror` is set.
`ClaudeAgentOptions.session_store` adds that flag automatically. The query
read loop consumes `transcript_mirror` frames internally, flushes pending
entries before yielding a `result`, and reports append failures as
`mirror_error` system messages.

## Store-backed resume

When `session_store` is set with `resume` or `continue_conversation`, the SDK
loads matching entries from the store into a temporary Claude config directory
and sets `CLAUDE_CONFIG_DIR` for the subprocess. The materialized directory is
removed when `QueryHandle::close()` or `ClaudeSDKClient::disconnect()` runs.

Explicit `resume` only materializes valid UUID session IDs. `continue` picks
the newest non-sidechain session from `SessionStore::list_sessions()`.

## Validation

Run:

```bash
bash skills/skill/scripts/run-validation.sh
```

That script runs formatting, clippy, full tests, and the ignored live e2e
tests. The live tests require a working `claude` binary plus local
authentication.

## Session management APIs

```rust
use claude_agent_sdk::{
    list_sessions, get_session_info, get_session_messages, rename_session,
    tag_session, fork_session,
};

let sessions = list_sessions(Some(std::path::Path::new(".")), Some(20), 0, true);
if let Some(first) = sessions.first() {
    let info = get_session_info(&first.session_id, Some(std::path::Path::new(".")));
    let messages = get_session_messages(&first.session_id, Some(std::path::Path::new(".")), None, 0);
    rename_session(&first.session_id, "Useful investigation", Some(std::path::Path::new(".")))?;
    tag_session(&first.session_id, Some("research"), Some(std::path::Path::new(".")))?;
    let fork = fork_session(&first.session_id, Some(std::path::Path::new(".")), None, None)?;
    println!("forked {}", fork.session_id);
}
```

Store-backed equivalents use `_from_store` for reads and `_via_store` for
mutations. `import_session_to_store` recursively imports local JSONL
transcripts from a Claude projects directory.

## SDK MCP builders

```rust
use claude_agent_sdk::{create_sdk_mcp_server, tool};
use serde_json::{json, Value};

let add = tool(
    "add",
    "Add two numbers",
    json!({
        "type": "object",
        "properties": {
            "a": {"type": "number"},
            "b": {"type": "number"}
        },
        "required": ["a", "b"]
    }),
    |args| async move {
        let a = args.get("a").and_then(Value::as_f64).unwrap_or(0.0);
        let b = args.get("b").and_then(Value::as_f64).unwrap_or(0.0);
        json!({"content": [{"type": "text", "text": (a + b).to_string()}]})
    },
);

let server = create_sdk_mcp_server("calc", "1.0.0", vec![add]);
// Insert server.config into ClaudeAgentOptions.mcp_servers and register
// server.handler on QueryConfig via QueryConfig::with_sdk_mcp_server(...).
```
