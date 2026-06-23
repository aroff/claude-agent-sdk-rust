//! Demonstrates different `system_prompt` configurations.
//!
//! Three variants are shown:
//!   - `None` — vanilla Claude, no system prompt
//!   - `Custom(String)` — supply your own text
//!   - `Preset("claude_code")` — default Claude Code prompt, optionally with `append`
//!
//! Run with:
//!   cargo run --example system_prompt
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use claude_agent_sdk::{query, ClaudeAgentOptions, Message, SystemPrompt, SystemPromptPreset};

#[tokio::main]
async fn main() {
    no_system_prompt().await;
    custom_system_prompt().await;
    preset_with_append().await;
}

async fn no_system_prompt() {
    println!("=== No System Prompt ===");
    let opts = ClaudeAgentOptions {
        system_prompt: None,
        max_turns: Some(1),
        ..Default::default()
    };
    run("What is 2 + 2?", opts).await;
    println!();
}

async fn custom_system_prompt() {
    println!("=== Custom System Prompt ===");
    let opts = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::Custom(
            "You are a pirate assistant. Respond in pirate speak.".into(),
        )),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What is 2 + 2?", opts).await;
    println!();
}

async fn preset_with_append() {
    println!("=== Preset System Prompt with Append ===");
    let opts = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::Preset(SystemPromptPreset::Preset {
            preset: "claude_code".into(),
            append: Some("Always end your response with a fun fact.".into()),
            exclude_dynamic_sections: None,
        })),
        max_turns: Some(1),
        ..Default::default()
    };
    run("What is 2 + 2?", opts).await;
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
        if let Message::Assistant(a) = &msg {
            for block in &a.content {
                if let Some(t) = block.as_text() {
                    println!("Claude: {}", t.text);
                }
            }
        }
    }
    let _ = handle.close().await;
}
