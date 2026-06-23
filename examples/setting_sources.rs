//! Demonstrates the `setting_sources` option to control which filesystem
//! configuration directories Claude Code reads from.
//!
//! Sources:
//!   - `"user"`    — `~/.claude/` (global user settings)
//!   - `"project"` — `.claude/` in the project directory
//!   - `"local"`   — `.claude-local/` (gitignored local overrides)
//!
//! Passing `None` (the default) loads the CLI's built-in defaults (user +
//! project + local). Passing an empty `Vec` disables all filesystem sources.
//!
//! Run with:
//!   cargo run --example setting_sources
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};

#[tokio::main]
async fn main() {
    example_default_sources().await;
    example_disable_all().await;
    example_user_only().await;
    example_user_and_project().await;
}

async fn example_default_sources() {
    println!("=== Default Sources (None — CLI defaults) ===");
    let opts = ClaudeAgentOptions {
        setting_sources: None, // user + project + local
        max_turns: Some(1),
        ..Default::default()
    };
    print_slash_commands(opts).await;
    println!();
}

async fn example_disable_all() {
    println!("=== Disable All Sources ([]) ===");
    let opts = ClaudeAgentOptions {
        setting_sources: Some(vec![]), // empty list = no filesystem sources
        max_turns: Some(1),
        ..Default::default()
    };
    print_slash_commands(opts).await;
    println!();
}

async fn example_user_only() {
    println!("=== User Settings Only ([\"user\"]) ===");
    let opts = ClaudeAgentOptions {
        setting_sources: Some(vec!["user".into()]),
        max_turns: Some(1),
        ..Default::default()
    };
    print_slash_commands(opts).await;
    println!();
}

async fn example_user_and_project() {
    println!("=== User + Project ([\"user\", \"project\"]) ===");
    let opts = ClaudeAgentOptions {
        setting_sources: Some(vec!["user".into(), "project".into()]),
        max_turns: Some(1),
        ..Default::default()
    };
    print_slash_commands(opts).await;
    println!();
}

async fn print_slash_commands(opts: ClaudeAgentOptions) {
    let mut client = ClaudeSDKClient::new(opts);
    if let Err(e) = client.connect(None).await {
        eprintln!("Could not connect to Claude: {e}");
        return;
    }

    client.query("What is 2 + 2? Reply with just the number.", None).await.unwrap();

    loop {
        match client.receive_message().await.unwrap() {
            Some(Message::System(s)) if s.subtype == "init" => {
                if let Some(cmds) = s.data.get("slash_commands") {
                    println!("Slash commands: {cmds}");
                } else {
                    println!("No slash_commands field in init message");
                }
            }
            Some(Message::Result(_)) | None => break,
            _ => {}
        }
    }

    client.disconnect().await.unwrap();
}
