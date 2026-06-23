//! Interactive client for multi-turn conversations with Claude Code.
//!
//! Mirrors the Python `ClaudeSDKClient`. Unlike [`query()`] (one-shot),
//! `ClaudeSDKClient` keeps the subprocess alive across multiple turns,
//! supports streaming input, interrupts, dynamic permission changes,
//! MCP server management, and all control-protocol methods.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::control::{Query, QueryConfig};
use crate::error::ClaudeSdkError;
use crate::message_parser::parse_message;
use crate::options::ClaudeAgentOptions;
use crate::session::{
    apply_materialized_options, build_mirror_batcher, materialize_resume_session,
    MaterializedResume,
};
use crate::transport::{SubprocessCLITransport, Transport};
use crate::types::{Message, PermissionMode};

/// Interactive client for bidirectional, multi-turn conversations.
///
/// Unlike [`query`](crate::query), the client maintains a live subprocess
/// across multiple turns, enabling follow-up messages, interrupts, and
/// dynamic control.
///
/// # Example
/// ```no_run
/// # async fn run() -> Result<(), claude_agent_sdk::ClaudeSdkError> {
/// use claude_agent_sdk::{ClaudeSDKClient, ClaudeAgentOptions, Message};
///
/// let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
/// client.connect(None).await?;
///
/// // Turn 1
/// client.query("What is 2+2?", None).await?;
/// while let Some(msg) = client.receive_response().await? {
///     if let Message::Result(_) = &msg { break; }
/// }
///
/// // Turn 2 — same session
/// client.query("Now multiply that by 3", None).await?;
/// while let Some(msg) = client.receive_response().await? {
///     if let Message::Assistant(a) = &msg {
///         for b in &a.content {
///             if let Some(t) = b.as_text() { println!("{}", t.text); }
///         }
///     }
/// }
///
/// client.disconnect().await?;
/// # Ok(())
/// # }
/// ```
pub struct ClaudeSDKClient {
    options: ClaudeAgentOptions,
    config: QueryConfig,
    query: Option<Arc<Mutex<Query>>>,
    rx: Option<mpsc::Receiver<Result<Value, ClaudeSdkError>>>,
    materialized: Option<MaterializedResume>,
}

impl ClaudeSDKClient {
    /// Create a new client with the given options and default control config.
    pub fn new(options: ClaudeAgentOptions) -> Self {
        Self {
            options,
            config: QueryConfig::default(),
            query: None,
            rx: None,
            materialized: None,
        }
    }

    /// Create a new client with an explicit control-protocol config (hooks,
    /// can_use_tool, agents, etc.).
    pub fn with_config(options: ClaudeAgentOptions, config: QueryConfig) -> Self {
        Self {
            options,
            config,
            query: None,
            rx: None,
            materialized: None,
        }
    }

    /// Connect to Claude. Spawns the subprocess, runs the initialize
    /// handshake, and prepares for message exchange.
    ///
    /// Pass an optional initial prompt to send immediately after connect.
    /// For interactive use, pass `None` and use [`query`](Self::query) later.
    pub async fn connect(&mut self, initial_prompt: Option<&str>) -> Result<(), ClaudeSdkError> {
        // When can_use_tool is set, auto-configure permission_prompt_tool_name
        // to "stdio" so the CLI routes permission requests via control protocol.
        if self.config.can_use_tool.is_some() {
            if self.options.permission_prompt_tool_name.is_some() {
                return Err(ClaudeSdkError::new(
                    "can_use_tool callback cannot be used with permission_prompt_tool_name",
                ));
            }
            self.options.permission_prompt_tool_name = Some("stdio".into());
        }

        self.materialized = materialize_resume_session(&self.options).await?;
        let effective_options = self
            .materialized
            .as_ref()
            .map(|m| apply_materialized_options(&self.options, m))
            .unwrap_or_else(|| self.options.clone());

        let mut transport = SubprocessCLITransport::new(effective_options.clone());
        transport.connect().await?;

        let boxed: Box<dyn Transport> = Box::new(transport);
        let mut query_obj = Query::new(boxed, self.config.clone())?;
        if let Some(store) = self.options.session_store.clone() {
            let on_error = query_obj.mirror_error_callback();
            query_obj.set_transcript_mirror_batcher(build_mirror_batcher(
                store,
                self.materialized.as_ref(),
                Some(&effective_options.env),
                self.options.session_store_flush,
                on_error,
            ));
        }
        query_obj.start();
        if let Err(e) = query_obj.initialize().await {
            if let Some(materialized) = self.materialized.take() {
                materialized.cleanup().await;
            }
            return Err(e);
        }

        // Send initial prompt if provided.
        if let Some(prompt) = initial_prompt {
            query_obj.write_user_message(prompt).await?;
        }

        let rx = query_obj
            .take_receiver()
            .ok_or_else(|| ClaudeSdkError::new("message receiver already taken"))?;

        self.query = Some(Arc::new(Mutex::new(query_obj)));
        self.rx = Some(rx);
        Ok(())
    }

    /// Send a new user message in the conversation.
    ///
    /// After calling this, use [`receive_response`](Self::receive_response) or
    /// [`receive_messages`](Self::receive_messages) to read the reply.
    ///
    /// `session_id` defaults to `"default"` when `None`.
    pub async fn query(
        &mut self,
        prompt: &str,
        session_id: Option<&str>,
    ) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        let msg = json!({
            "type": "user",
            "session_id": session_id.unwrap_or("default"),
            "message": {"role": "user", "content": prompt},
            "parent_tool_use_id": Value::Null,
        });
        let payload = format!("{}\n", msg);
        // Access the writer through the Query's internal writer.
        q.lock().await.write_raw(&payload).await
    }

    /// Receive the next parsed message, or `None` when the stream ends.
    pub async fn receive_message(&mut self) -> Result<Option<Message>, ClaudeSdkError> {
        loop {
            let rx = self
                .rx
                .as_mut()
                .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
            match rx.recv().await {
                Some(Ok(value)) => {
                    let t = value.get("type").and_then(Value::as_str).unwrap_or("");
                    if t == "control_response" || t == "control_request" {
                        continue;
                    }
                    match parse_message(&value) {
                        Ok(Some(msg)) => return Ok(Some(msg)),
                        Ok(None) => continue,
                        Err(e) => return Err(ClaudeSdkError::new(e.message)),
                    }
                }
                Some(Err(e)) => return Err(e),
                None => return Ok(None),
            }
        }
    }

    /// Convenience wrapper: receive messages until and including a
    /// `ResultMessage`, then return. Yields one message at a time.
    pub async fn receive_response(&mut self) -> Result<Option<Message>, ClaudeSdkError> {
        let msg = self.receive_message().await?;
        if matches!(msg, Some(Message::Result(_))) {
            return Ok(msg);
        }
        Ok(msg)
    }

    /// Send an interrupt to the running conversation.
    pub async fn interrupt(&self) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.interrupt().await
    }

    /// Change the permission mode mid-conversation.
    pub async fn set_permission_mode(&self, mode: PermissionMode) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.set_permission_mode(mode).await
    }

    /// Change the model mid-conversation.
    pub async fn set_model(&self, model: Option<&str>) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.set_model(model).await
    }

    /// Rewind tracked files to a specific user message checkpoint.
    pub async fn rewind_files(&self, user_message_id: &str) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.rewind_files(user_message_id).await
    }

    /// Reconnect a disconnected or failed MCP server.
    pub async fn reconnect_mcp_server(&self, server_name: &str) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.reconnect_mcp_server(server_name).await
    }

    /// Enable or disable an MCP server.
    pub async fn toggle_mcp_server(
        &self,
        server_name: &str,
        enabled: bool,
    ) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.toggle_mcp_server(server_name, enabled).await
    }

    /// Stop a running task by ID.
    pub async fn stop_task(&self, task_id: &str) -> Result<(), ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.stop_task(task_id).await
    }

    /// Get current MCP server connection status.
    pub async fn get_mcp_status(&self) -> Result<Value, ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.get_mcp_status().await
    }

    /// Get a breakdown of current context window usage.
    pub async fn get_context_usage(&self) -> Result<Value, ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        q.lock().await.get_context_usage().await
    }

    /// Get server initialization info (commands, agents, output styles).
    pub async fn get_server_info(&self) -> Result<Option<Value>, ClaudeSdkError> {
        let q = self
            .query
            .as_ref()
            .ok_or_else(|| ClaudeSdkError::new("Not connected. Call connect() first."))?;
        Ok(q.lock().await.initialization_result().cloned())
    }

    /// Disconnect from Claude. Closes the subprocess and releases resources.
    pub async fn disconnect(&mut self) -> Result<(), ClaudeSdkError> {
        if let Some(rx) = self.rx.as_mut() {
            rx.close();
        }
        self.rx = None;
        if let Some(q_arc) = self.query.take() {
            q_arc.lock().await.flush_transcript_mirror().await;
            // If we're the sole owner, unwrap and close cleanly.
            if let Ok(mutex) = Arc::try_unwrap(q_arc) {
                // Extract Query from the Mutex via try_into_inner? No —
                // Mutex doesn't expose that. We need to lock and the Query's
                // close() takes self by value. Use a Option inside Mutex
                // pattern... simpler: just drop the Arc; kill_on_drop handles
                // the subprocess.
                drop(mutex);
            }
        }
        if let Some(materialized) = self.materialized.take() {
            materialized.cleanup().await;
        }
        Ok(())
    }
}

impl Drop for ClaudeSDKClient {
    fn drop(&mut self) {
        if let Some(q_arc) = self.query.take() {
            // Best-effort: abort the read task if the Arc is uniquely owned.
            if let Ok(query) = Arc::try_unwrap(q_arc) {
                // query.close() is async; we can't await in Drop.
                // The transport has kill_on_drop(true), so the child is
                // cleaned up by the OS. The read task is aborted when the
                // runtime drops the JoinHandle.
                drop(query);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_new_defaults() {
        let client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
        assert!(client.query.is_none());
        assert!(client.rx.is_none());
    }

    #[tokio::test]
    async fn operations_before_connect_error() {
        let client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
        let err = client.interrupt().await.unwrap_err();
        assert!(err.message.contains("Not connected"));
    }

    #[tokio::test]
    async fn query_before_connect_errors() {
        let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
        let err = client.query("hello", None).await.unwrap_err();
        assert!(err.message.contains("Not connected"));
    }

    #[tokio::test]
    async fn receive_message_before_connect_errors() {
        let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
        let err = client.receive_message().await.unwrap_err();
        assert!(err.message.contains("Not connected"));
    }

    #[tokio::test]
    async fn disconnect_without_connect_is_noop() {
        let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
        client.disconnect().await.unwrap();
        assert!(client.query.is_none());
    }
}
