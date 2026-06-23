//! Multi-turn conversation using `ClaudeSDKClient`.
//!
//! The client keeps the subprocess alive between turns so Claude remembers
//! prior context within the same session.
//!
//! Run with:
//!   cargo run --example multi_turn
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

#[tokio::main]
async fn main() {
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    let mut client = ClaudeSDKClient::new(opts);

    if let Err(e) = client.connect(None).await {
        eprintln!("Could not connect to Claude: {e}");
        return;
    }

    // Turn 1
    let q1 = "What is the capital of France? Reply with just the city name.";
    println!("User: {q1}");
    client.query(q1, None).await.unwrap();
    drain_until_result(&mut client).await;

    // Turn 2 — same session, Claude remembers the previous answer
    let q2 = "What language do people speak in that city? Reply with one word.";
    println!("\nUser: {q2}");
    client.query(q2, None).await.unwrap();
    drain_until_result(&mut client).await;

    client.disconnect().await.unwrap();
}

async fn drain_until_result(client: &mut ClaudeSDKClient) {
    loop {
        match client.receive_message().await.unwrap() {
            Some(Message::Assistant(a)) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        println!("Claude: {}", t.text);
                    }
                }
            }
            Some(Message::Result(_)) | None => break,
            _ => {}
        }
    }
}
