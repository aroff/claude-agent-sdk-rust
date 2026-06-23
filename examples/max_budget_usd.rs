//! Demonstrates the `max_budget_usd` option for cost control.
//!
//! Budget checking happens after each API call completes, so the actual cost
//! may slightly exceed the limit. When exceeded, `ResultMessage.subtype` is
//! `"error_max_budget_usd"` and `is_error` is `true`.
//!
//! Run with:
//!   cargo run --example max_budget_usd
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

#[tokio::main]
async fn main() {
    without_budget().await;
    with_reasonable_budget().await;
    with_tight_budget().await;

    println!(
        "\nNote: budget checking happens after each API call, so final cost\n\
         may slightly exceed the specified limit."
    );
}

async fn without_budget() {
    println!("=== Without Budget Limit ===");
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    run("What is 2 + 2? Reply with just the number.", opts).await;
    println!();
}

async fn with_reasonable_budget() {
    println!("=== With Reasonable Budget ($0.10) ===");
    let opts = ClaudeAgentOptions {
        max_budget_usd: Some(0.10),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What is 2 + 2? Reply with just the number.", opts).await;
    println!();
}

async fn with_tight_budget() {
    println!("=== With Tight Budget ($0.0001) ===");
    let opts = ClaudeAgentOptions {
        max_budget_usd: Some(0.0001),
        max_turns: Some(2),
        ..Default::default()
    };
    run("Read the README.md file and summarize it.", opts).await;
    println!();
}

async fn run(prompt: &str, opts: ClaudeAgentOptions) {
    let mut handle = match query(prompt, opts).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };
    while let Ok(Some(msg)) = handle.next_message().await {
        match &msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        println!("Claude: {}", t.text);
                    }
                }
            }
            Message::Result(r) => {
                if let Some(cost) = r.total_cost_usd {
                    if cost > 0.0 {
                        println!("Cost: ${cost:.4}");
                    }
                }
                println!("Status: {}", r.subtype);
                if r.subtype == "error_max_budget_usd" {
                    println!("Budget limit exceeded!");
                }
            }
            _ => {}
        }
    }
    let _ = handle.close().await;
}
