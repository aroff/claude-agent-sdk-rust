//! In-process SDK MCP calculator server.
//!
//! Demonstrates wiring Rust async functions as MCP tools that Claude can call
//! without spawning a separate server process. The tools run in the same Tokio
//! runtime as the SDK client.
//!
//! Run with:
//!   cargo run --example sdk_mcp_calculator
//!
//! Requires:
//!   - Claude Code CLI: curl -fsSL https://claude.ai/install.sh | bash
//!   - ANTHROPIC_API_KEY in your environment
use std::collections::BTreeMap;

use claude_agent_sdk::{
    create_sdk_mcp_server, tool, ClaudeAgentOptions, ClaudeSDKClient, McpServers, Message,
    QueryConfig,
};
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    // --- Define tools ---

    let add = tool(
        "add",
        "Add two numbers",
        json!({
            "type": "object",
            "properties": { "a": {"type": "number"}, "b": {"type": "number"} },
            "required": ["a", "b"]
        }),
        |args: Value| async move {
            let a = args["a"].as_f64().unwrap_or(0.0);
            let b = args["b"].as_f64().unwrap_or(0.0);
            json!({"content": [{"type": "text", "text": format!("{a} + {b} = {}", a + b)}]})
        },
    );

    let multiply = tool(
        "multiply",
        "Multiply two numbers",
        json!({
            "type": "object",
            "properties": { "a": {"type": "number"}, "b": {"type": "number"} },
            "required": ["a", "b"]
        }),
        |args: Value| async move {
            let a = args["a"].as_f64().unwrap_or(0.0);
            let b = args["b"].as_f64().unwrap_or(0.0);
            json!({"content": [{"type": "text", "text": format!("{a} × {b} = {}", a * b)}]})
        },
    );

    // --- Build the in-process MCP server ---

    let server = create_sdk_mcp_server("calc", "1.0.0", vec![add, multiply]);

    // The server config tells the CLI that "calc" is an SDK-type server
    // (no separate process needed). The handler is registered on QueryConfig
    // so the SDK routes JSON-RPC calls to our Rust functions.
    let server_config = server.config.clone();
    let qconfig = QueryConfig::default().with_sdk_mcp_server("calc", server);

    let mut mcp_map: BTreeMap<String, serde_json::Map<String, Value>> = BTreeMap::new();
    mcp_map.insert("calc".into(), server_config);

    let opts = ClaudeAgentOptions {
        mcp_servers: McpServers::Map(mcp_map),
        // Pre-approve the tools so Claude doesn't ask for permission
        allowed_tools: vec!["mcp__calc__add".into(), "mcp__calc__multiply".into()],
        max_turns: Some(3),
        ..Default::default()
    };

    // --- Connect client ---

    let mut client = ClaudeSDKClient::with_config(opts, qconfig);
    if let Err(e) = client.connect(None).await {
        eprintln!("Could not connect to Claude: {e}");
        return;
    }

    // --- Run example prompts ---

    let prompts = [
        "What is 15 + 27? Use your tools.",
        "What is 6 multiplied by 7? Use your tools.",
    ];

    for prompt in &prompts {
        println!("\nUser: {prompt}");
        client.query(prompt, None).await.unwrap();

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

    client.disconnect().await.unwrap();
}
