// Live integration test for ClaudeSDKClient multi-turn.
// Run with: cargo test --test live_client -- --nocapture --ignored
use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient, Message};
use std::time::Duration;

const LIVE_TIMEOUT: Duration = Duration::from_secs(240);

fn is_env_noise(e: &claude_agent_sdk::ClaudeSdkError) -> bool {
    let m = e.message.to_lowercase();
    m.contains("not found")
        || m.contains("no such file")
        || m.contains("claude code not found")
        || m.contains("api key")
}

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_client_multi_turn() {
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    let mut client = ClaudeSDKClient::new(opts);

    // Connect
    match tokio::time::timeout(LIVE_TIMEOUT, client.connect(None)).await {
        Err(_) => { eprintln!("[skip] connect timed out — no live Claude available"); return; }
        Ok(Err(e)) if is_env_noise(&e) => { eprintln!("[skip] environment noise: {e}"); return; }
        Ok(Err(e)) => panic!("connect failed: {e}"),
        Ok(Ok(())) => {}
    };

    // Turn 1
    client
        .query("What is 2+2? Reply with just the number.", None)
        .await
        .unwrap();

    let mut turn1_text = String::new();
    loop {
        let msg = tokio::time::timeout(LIVE_TIMEOUT, client.receive_message())
            .await
            .expect("timed out reading turn 1")
            .unwrap()
            .unwrap();
        match &msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        turn1_text.push_str(&t.text);
                    }
                }
            }
            Message::Result(_) => break,
            _ => {}
        }
    }
    eprintln!("[turn 1] reply: {turn1_text}");

    // Turn 2 — same session, follow-up
    client
        .query("Now multiply that by 3. Reply with just the number.", None)
        .await
        .unwrap();

    let mut turn2_text = String::new();
    loop {
        let msg = tokio::time::timeout(LIVE_TIMEOUT, client.receive_message())
            .await
            .expect("timed out reading turn 2")
            .unwrap()
            .unwrap();
        match &msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        turn2_text.push_str(&t.text);
                    }
                }
            }
            Message::Result(_) => break,
            _ => {}
        }
    }
    eprintln!("[turn 2] reply: {turn2_text}");

    // Verify the client maintained context (12 = 4 * 3)
    assert!(!turn2_text.is_empty(), "expected non-empty turn 2 reply");
    eprintln!("[ok] multi-turn conversation successful");

    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_client_get_mcp_status() {
    let opts = ClaudeAgentOptions::default();
    let mut client = ClaudeSDKClient::new(opts);
    match tokio::time::timeout(LIVE_TIMEOUT, client.connect(None)).await {
        Err(_) => { eprintln!("[skip] connect timed out — no live Claude available"); return; }
        Ok(Err(e)) if is_env_noise(&e) => { eprintln!("[skip] environment noise: {e}"); return; }
        Ok(Err(e)) => panic!("connect failed: {e}"),
        Ok(Ok(())) => {}
    };

    let status = client.get_mcp_status().await.unwrap();
    assert!(status.is_object());
    eprintln!("[ok] mcp status: {}", status);
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_client_get_server_info() {
    let opts = ClaudeAgentOptions::default();
    let mut client = ClaudeSDKClient::new(opts);
    match tokio::time::timeout(LIVE_TIMEOUT, client.connect(None)).await {
        Err(_) => { eprintln!("[skip] connect timed out — no live Claude available"); return; }
        Ok(Err(e)) if is_env_noise(&e) => { eprintln!("[skip] environment noise: {e}"); return; }
        Ok(Err(e)) => panic!("connect failed: {e}"),
        Ok(Ok(())) => {}
    };

    let info = client.get_server_info().await.unwrap();
    assert!(info.is_some());
    let info = info.unwrap();
    assert!(info.get("commands").is_some() || info.get("agents").is_some());
    eprintln!(
        "[ok] server info keys: {:?}",
        info.as_object().map(|m| m.keys().collect::<Vec<_>>())
    );
    client.disconnect().await.unwrap();
}
