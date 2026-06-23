// Live integration test: spawns real claude, parses all messages.
// Run with: cargo test --test live_query -- --nocapture --ignored
use claude_agent_sdk::{query, ClaudeAgentOptions, Message};
use std::time::Duration;

const LIVE_TIMEOUT: Duration = Duration::from_secs(240);

#[tokio::test]
#[ignore = "requires local claude binary + API key"]
async fn live_query_round_trip() {
    let opts = ClaudeAgentOptions {
        max_turns: Some(1),
        ..Default::default()
    };
    let mut handle = tokio::time::timeout(
        LIVE_TIMEOUT,
        query("What is 2+2? Reply with just the number.", opts),
    )
    .await
    .expect("query() timed out")
    .expect("query() failed");

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
