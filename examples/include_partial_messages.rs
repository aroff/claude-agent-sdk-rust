//! Stream partial (incremental) messages using `include_partial_messages`.
//!
//! With this option enabled the CLI emits `StreamEvent` messages between
//! `Assistant` messages, carrying incremental text deltas as Claude generates
//! its response. Useful for building real-time UIs or monitoring tool
//! progress before a full turn completes.
//!
//! Run with:
//!   cargo run --example include_partial_messages
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

#[tokio::main]
async fn main() {
    let opts = ClaudeAgentOptions {
        include_partial_messages: true,
        max_turns: Some(1),
        ..Default::default()
    };

    let mut client = ClaudeSDKClient::new(opts);
    if let Err(e) = client.connect(None).await {
        eprintln!("Could not connect to Claude: {e}");
        return;
    }

    let prompt = "Think of three jokes, then tell the best one.";
    println!("User: {prompt}\n");
    client.query(prompt, None).await.unwrap();

    let mut stream_events = 0usize;
    loop {
        match client.receive_message().await.unwrap() {
            Some(Message::StreamEvent(ev)) => {
                stream_events += 1;
                // The `event` field contains the raw incremental delta from the CLI
                if let Some(text) = ev.event.get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                {
                    print!("{text}");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            }
            Some(Message::Assistant(a)) => {
                // Full assistant message arrives after streaming finishes
                if stream_events == 0 {
                    for block in &a.content {
                        if let Some(t) = block.as_text() {
                            println!("{}", t.text);
                        }
                    }
                } else {
                    println!(); // newline after streamed output
                }
            }
            Some(Message::Result(r)) => {
                if let Some(cost) = r.total_cost_usd {
                    if cost > 0.0 {
                        println!("\nCost: ${cost:.4}");
                    }
                }
                println!("Stream events received: {stream_events}");
                break;
            }
            None => break,
            _ => {}
        }
    }

    client.disconnect().await.unwrap();
}
