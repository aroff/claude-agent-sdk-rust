//! Demonstrates the `tools` option and reading the active tool list from
//! the `System(init)` message.
//!
//! Three variants:
//!   - `Tools::List([...])` — specific named tools only
//!   - `Tools::List([])` — empty list disables all built-in tools
//!   - `Tools::Preset("claude_code")` — full default Claude Code tool set
//!
//! Run with:
//!   cargo run --example tools_option
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{query, ClaudeAgentOptions, Message, Tools, ToolsPreset};

#[tokio::main]
async fn main() {
    tools_list().await;
    tools_empty().await;
    tools_preset().await;
}

async fn tools_list() {
    println!("=== Tools List: [Read, Glob, Grep] ===");
    let opts = ClaudeAgentOptions {
        tools: Some(Tools::List(vec![
            "Read".into(),
            "Glob".into(),
            "Grep".into(),
        ])),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What tools do you have available? List them briefly.", opts).await;
    println!();
}

async fn tools_empty() {
    println!("=== Tools Empty (all built-ins disabled) ===");
    let opts = ClaudeAgentOptions {
        tools: Some(Tools::List(vec![])),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What tools do you have available? List them briefly.", opts).await;
    println!();
}

async fn tools_preset() {
    println!("=== Tools Preset: claude_code ===");
    let opts = ClaudeAgentOptions {
        tools: Some(Tools::Preset(ToolsPreset::Preset {
            preset: "claude_code".into(),
        })),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What tools do you have available? List them briefly.", opts).await;
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
            Message::System(s) if s.subtype == "init" => {
                if let Some(tools) = s.data.get("tools") {
                    if let Some(arr) = tools.as_array() {
                        let names: Vec<&str> = arr
                            .iter()
                            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
                            .collect();
                        println!("Active tools ({} total): {:?}", names.len(), &names[..names.len().min(6)]);
                        if names.len() > 6 { println!("  ... and {} more", names.len() - 6); }
                    }
                }
            }
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        println!("Claude: {}", t.text);
                    }
                }
            }
            _ => {}
        }
    }
    let _ = handle.close().await;
}
