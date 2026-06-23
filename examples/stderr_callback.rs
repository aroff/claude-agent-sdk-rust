//! Capture CLI stderr output via a callback.
//!
//! The `stderr` field on `ClaudeAgentOptions` accepts an `Arc<dyn Fn(&str)>`
//! that is called with each line the CLI writes to stderr. Useful for
//! surfacing warnings and errors from the subprocess without mixing them
//! into your structured message stream.
//!
//! Run with:
//!   cargo run --example stderr_callback
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use std::sync::{Arc, Mutex};

use claude_agent_sdk::{query, ClaudeAgentOptions, Message};

#[tokio::main]
async fn main() {
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = Arc::clone(&captured);

    let opts = ClaudeAgentOptions {
        stderr: Some(Arc::new(move |line: &str| {
            captured_clone.lock().unwrap().push(line.to_string());
            if line.contains("[ERROR]") || line.contains("error") {
                eprintln!("CLI stderr: {line}");
            }
        })),
        max_turns: Some(1),
        ..Default::default()
    };

    println!("Running query with stderr capture...");

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

    let lines = captured.lock().unwrap();
    println!("\nCaptured {} stderr line(s)", lines.len());
    if let Some(first) = lines.first() {
        println!("First line: {}", &first[..first.len().min(100)]);
    }
}
