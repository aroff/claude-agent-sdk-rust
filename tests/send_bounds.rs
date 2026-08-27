//! Regression: the interactive client must be usable from spawned tasks.
//!
//! `ClaudeSDKClient::query`/`interrupt` hold the internal `Mutex<Query>`
//! guard across awaits, so their futures are `Send` only if `Query: Sync` —
//! which requires the boxed transport halves to be `Sync`. Without the
//! `Sync` bounds on `TransportReader`/`TransportWriter`, any daemon that
//! drives a session from a `tokio::spawn`ed actor fails to compile.

use claude_agent_sdk::{ClaudeAgentOptions, ClaudeSDKClient};

fn assert_send<T: Send>(_: T) {}

#[test]
fn interactive_client_turn_future_is_send() {
    let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
    assert_send(async move {
        let _ = client.query("hello", None).await;
        let _ = client.receive_response().await;
        let _ = client.interrupt().await;
        let _ = client.disconnect().await;
    });
}
