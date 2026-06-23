// Live integration test: spawns real claude, parses all messages.
// Run with: cargo test --test live_query -- --nocapture --ignored
use claude_agent_sdk::{query, query_with_messages, ClaudeAgentOptions, Message};
use futures::stream;
use serde_json::json;
use std::time::Duration;

const LIVE_TIMEOUT: Duration = Duration::from_secs(240);

/// Return true when the error is environment noise (CLI not installed, no API
/// key, network timeout) rather than an SDK code path failure. Live tests
/// skip gracefully so CI without a real API key doesn't report false negatives.
fn is_env_noise(e: &claude_agent_sdk::ClaudeSdkError) -> bool {
    let m = e.message.to_lowercase();
    m.contains("not found")
        || m.contains("no such file")
        || m.contains("claude code not found")
        || m.contains("api key")
}

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_query_round_trip() {
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    let mut handle = match tokio::time::timeout(
        LIVE_TIMEOUT,
        query("What is 2+2? Reply with just the number.", opts),
    )
    .await
    {
        Err(_) => {
            eprintln!("[skip] query() timed out — no live Claude available");
            return;
        }
        Ok(Err(e)) if is_env_noise(&e) => {
            eprintln!("[skip] environment noise: {e}");
            return;
        }
        Ok(Err(e)) => panic!("query() failed: {e}"),
        Ok(Ok(h)) => h,
    };

    let mut got_text = String::new();
    let mut got_result = false;
    loop {
        let result = tokio::time::timeout(LIVE_TIMEOUT, handle.next_message())
            .await
            .expect("timed out reading messages");
        let msg = match result {
            Ok(Some(msg)) => msg,
            Ok(None) => break,
            Err(e) => panic!("error: {e}"),
        };
        match &msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        got_text.push_str(&t.text);
                    }
                }
            }
            Message::Result(_) => {
                got_result = true;
                break;
            }
            _ => {}
        }
    }
    handle.close().await.unwrap();
    assert!(got_result, "expected a Result message");
    assert!(!got_text.is_empty(), "expected assistant text");
    eprintln!("[ok] assistant replied: {got_text}");
}

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_query_with_messages_stream() {
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };

    let messages = stream::iter(vec![json!({
        "type": "user",
        "session_id": "",
        "message": {"role": "user", "content": "What is 3+3? Reply with just the number."},
        "parent_tool_use_id": null,
    })]);

    let mut handle = match tokio::time::timeout(
        LIVE_TIMEOUT,
        query_with_messages(messages, opts),
    )
    .await
    {
        Err(_) => {
            eprintln!("[skip] query_with_messages() timed out — no live Claude available");
            return;
        }
        Ok(Err(e)) if is_env_noise(&e) => {
            eprintln!("[skip] environment noise: {e}");
            return;
        }
        Ok(Err(e)) => panic!("query_with_messages() failed: {e}"),
        Ok(Ok(h)) => h,
    };

    let mut got_text = String::new();
    let mut got_result = false;
    loop {
        let result = tokio::time::timeout(LIVE_TIMEOUT, handle.next_message())
            .await
            .expect("timed out reading messages");
        let msg = match result {
            Ok(Some(msg)) => msg,
            Ok(None) => break,
            Err(e) => panic!("error: {e}"),
        };
        match &msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let Some(t) = block.as_text() {
                        got_text.push_str(&t.text);
                    }
                }
            }
            Message::Result(_) => {
                got_result = true;
                break;
            }
            _ => {}
        }
    }
    handle.close().await.unwrap();
    assert!(got_result, "expected a Result message");
    assert!(!got_text.is_empty(), "expected assistant text");
    eprintln!("[ok] stream query replied: {got_text}");
}
