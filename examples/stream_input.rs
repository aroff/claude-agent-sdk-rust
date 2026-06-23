//! Stream a pre-built sequence of messages with `query_with_messages`.
//!
//! Mirrors the Python SDK's `query(prompt=async_iterable)` path. Instead of
//! a single string, you supply a `futures::Stream` of JSON message objects —
//! each is serialised and written to the CLI's stdin as it arrives.
//!
//! Run with:
//!   cargo run --example stream_input
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{query_with_messages, ClaudeAgentOptions, Message};
use futures::stream;
use serde_json::json;

#[tokio::main]
async fn main() {
    println!("=== Stream Input Example ===");

    // Each item in the stream is a raw JSON message object in the format
    // expected by the Claude CLI's stream-json input protocol.
    let messages = stream::iter(vec![json!({
        "type": "user",
        "session_id": "",
        "message": {
            "role": "user",
            "content": "What is 6 × 7? Reply with just the number."
        },
        "parent_tool_use_id": null,
    })]);

    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };

    let mut handle = match query_with_messages(messages, opts).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };

    while let Ok(Some(msg)) = handle.next_message().await {
        if let Message::Assistant(a) = &msg {
            for block in &a.content {
                if let Some(t) = block.as_text() {
                    println!("Claude: {}", t.text);
                }
            }
        }
        if let Message::Result(r) = &msg {
            if let Some(cost) = r.total_cost_usd {
                if cost > 0.0 {
                    println!("Cost: ${cost:.4}");
                }
            }
        }
    }
    let _ = handle.close().await;
}
