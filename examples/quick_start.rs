//! Quick-start: one-shot `query()` with basic options and result handling.
//!
//! Run with:
//!   cargo run --example quick_start
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{query, ClaudeAgentOptions, Message, SystemPrompt};

#[tokio::main]
async fn main() {
    basic_query().await;
    with_options().await;
}

async fn basic_query() {
    println!("=== Basic Query ===");

    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    let mut handle = match query("What is 2 + 2? Reply with just the number.", opts).await {
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
    }
    let _ = handle.close().await;
    println!();
}

async fn with_options() {
    println!("=== With Options ===");

    let opts = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::Custom(
            "You are a concise assistant. Keep answers to one sentence.".into(),
        )),
        max_turns: Some(1),
        ..Default::default()
    };

    let mut handle = match query("What is Rust?", opts).await {
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
