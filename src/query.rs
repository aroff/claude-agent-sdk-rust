//! High-level query API: one-shot and interactive conversations with Claude.
//!
//! Mirrors the Python `query()` function.

use serde_json::Value;
use tokio::sync::mpsc;

use crate::control::{Query, QueryConfig};
use crate::error::ClaudeSdkError;
use crate::message_parser::parse_message;
use crate::options::ClaudeAgentOptions;
use crate::session::{
    apply_materialized_options, build_mirror_batcher, materialize_resume_session,
    MaterializedResume,
};
use crate::transport::{SubprocessCLITransport, Transport};
use crate::types::Message;

/// A handle to a running query. Yields parsed [`Message`]s.
///
/// Created by [`query`] or [`query_with_messages`]. Drop it (or call
/// [`close`]) to terminate the underlying subprocess.
///
/// [`close`]: QueryHandle::close
pub struct QueryHandle {
    query: Option<Query>,
    rx: mpsc::Receiver<Result<Value, ClaudeSdkError>>,
    materialized: Option<MaterializedResume>,
    /// Optional background write task (used by the streaming query path).
    write_task: Option<tokio::task::JoinHandle<()>>,
}

impl QueryHandle {
    /// Receive the next parsed message, or `None` when the stream ends.
    pub async fn next_message(&mut self) -> Result<Option<Message>, ClaudeSdkError> {
        loop {
            match self.rx.recv().await {
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

    /// Send an interrupt to the running conversation.
    pub async fn interrupt(&mut self) -> Result<(), ClaudeSdkError> {
        match &self.query {
            Some(q) => q.interrupt().await,
            None => Err(ClaudeSdkError::new("query already closed")),
        }
    }

    /// Change the permission mode mid-conversation.
    pub async fn set_permission_mode(
        &mut self,
        mode: crate::types::PermissionMode,
    ) -> Result<(), ClaudeSdkError> {
        match &self.query {
            Some(q) => q.set_permission_mode(mode).await,
            None => Err(ClaudeSdkError::new("query already closed")),
        }
    }

    /// Gracefully close the query and its transport.
    pub async fn close(mut self) -> Result<(), ClaudeSdkError> {
        // Abort the streaming write task if present (no-op for one-shot queries).
        if let Some(task) = self.write_task.take() {
            task.abort();
            let _ = task.await;
        }
        // Drop the receiver first so the read loop isn't blocked.
        self.rx.close();
        if let Some(q) = self.query.take() {
            q.close().await?;
        }
        if let Some(materialized) = self.materialized.take() {
            materialized.cleanup().await;
        }
        Ok(())
    }
}

/// Start a one-shot query against Claude Code.
///
/// Spawns the `claude` CLI subprocess, runs the initialize handshake, sends
/// the prompt, and returns a [`QueryHandle`] that yields parsed messages.
///
/// # Example
/// ```no_run
/// # async fn run() -> Result<(), claude_agent_sdk::ClaudeSdkError> {
/// use claude_agent_sdk::{query, ClaudeAgentOptions, Message, ContentBlock};
///
/// let mut handle = query("What is 2+2?", ClaudeAgentOptions::default()).await?;
/// while let Some(msg) = handle.next_message().await? {
///     if let Message::Assistant(a) = &msg {
///         for block in &a.content {
///             if let Some(t) = block.as_text() {
///                 println!("{}", t.text);
///             }
///         }
///     }
/// }
/// handle.close().await?;
/// # Ok(())
/// # }
/// ```
pub async fn query(
    prompt: impl AsRef<str>,
    options: ClaudeAgentOptions,
) -> Result<QueryHandle, ClaudeSdkError> {
    query_with_config(prompt, options, QueryConfig::default()).await
}

/// Like [`query`] but with an explicit control-protocol config (hooks,
/// can_use_tool callback, agents, etc.).
pub async fn query_with_config(
    prompt: impl AsRef<str>,
    options: ClaudeAgentOptions,
    config: QueryConfig,
) -> Result<QueryHandle, ClaudeSdkError> {
    if config.can_use_tool.is_some() {
        return Err(ClaudeSdkError::new(
            "can_use_tool callback requires streaming mode; use the streaming query API instead of a string prompt",
        ));
    }
    if config.can_use_tool.is_some() && options.permission_prompt_tool_name.is_some() {
        return Err(ClaudeSdkError::new(
            "can_use_tool callback cannot be used with permission_prompt_tool_name",
        ));
    }

    let materialized = materialize_resume_session(&options).await?;
    let effective_options = materialized
        .as_ref()
        .map(|m| apply_materialized_options(&options, m))
        .unwrap_or_else(|| options.clone());

    let mut transport = SubprocessCLITransport::new(effective_options.clone());
    transport.connect().await?;

    let boxed: Box<dyn Transport> = Box::new(transport);
    let mut query_obj = Query::new(boxed, config)?;
    if let Some(store) = options.session_store.clone() {
        let on_error = query_obj.mirror_error_callback();
        query_obj.set_transcript_mirror_batcher(build_mirror_batcher(
            store,
            materialized.as_ref(),
            Some(&effective_options.env),
            options.session_store_flush,
            on_error,
        ));
    }
    query_obj.start();
    if let Err(e) = query_obj.initialize().await {
        if let Some(materialized) = materialized.as_ref() {
            materialized.cleanup().await;
        }
        return Err(e);
    }

    query_obj.write_user_message(prompt.as_ref()).await?;
    // Close stdin unless the config needs it open for in-process MCP or hooks.
    // Without EOF, the CLI waits for more input and never emits its Result message.
    if !query_obj.needs_stdin_for_control() {
        query_obj.end_input().await?;
    }

    let rx = query_obj
        .take_receiver()
        .ok_or_else(|| ClaudeSdkError::new("message receiver already taken"))?;

    Ok(QueryHandle {
        query: Some(query_obj),
        rx,
        materialized,
        write_task: None,
    })
}

/// Start a streaming query against Claude Code.
///
/// Accepts an arbitrary [`futures::Stream`] of JSON values — each item is
/// serialised as one newline-delimited message on the CLI's stdin. This
/// mirrors the Python SDK's `query(prompt=async_iterable)` mode.
///
/// After the stream is exhausted stdin is closed, signalling to the CLI that
/// no further user input is coming. For one-shot string prompts use [`query`]
/// instead.
///
/// # Example
/// ```no_run
/// # async fn run() -> Result<(), claude_agent_sdk::ClaudeSdkError> {
/// use claude_agent_sdk::{query_with_messages, ClaudeAgentOptions, Message};
/// use futures::stream;
/// use serde_json::json;
///
/// let messages = stream::iter(vec![
///     json!({
///         "type": "user",
///         "session_id": "",
///         "message": {"role": "user", "content": "What is 2+2?"},
///         "parent_tool_use_id": null,
///     }),
/// ]);
/// let mut handle = query_with_messages(messages, ClaudeAgentOptions::default()).await?;
/// while let Some(msg) = handle.next_message().await? {
///     println!("{msg:?}");
/// }
/// handle.close().await?;
/// # Ok(())
/// # }
/// ```
pub async fn query_with_messages<S>(
    messages: S,
    options: ClaudeAgentOptions,
) -> Result<QueryHandle, ClaudeSdkError>
where
    S: futures::Stream<Item = Value> + Send + 'static,
{
    query_with_messages_and_config(messages, options, QueryConfig::default()).await
}

/// Like [`query_with_messages`] but with an explicit control-protocol config
/// (hooks, `can_use_tool` callback, agents, SDK MCP servers, etc.).
pub async fn query_with_messages_and_config<S>(
    messages: S,
    options: ClaudeAgentOptions,
    config: QueryConfig,
) -> Result<QueryHandle, ClaudeSdkError>
where
    S: futures::Stream<Item = Value> + Send + 'static,
{
    let materialized = materialize_resume_session(&options).await?;
    let effective_options = materialized
        .as_ref()
        .map(|m| apply_materialized_options(&options, m))
        .unwrap_or_else(|| options.clone());

    let mut transport = SubprocessCLITransport::new(effective_options.clone());
    transport.connect().await?;

    let boxed: Box<dyn Transport> = Box::new(transport);
    let mut query_obj = Query::new(boxed, config)?;
    if let Some(store) = options.session_store.clone() {
        let on_error = query_obj.mirror_error_callback();
        query_obj.set_transcript_mirror_batcher(build_mirror_batcher(
            store,
            materialized.as_ref(),
            Some(&effective_options.env),
            options.session_store_flush,
            on_error,
        ));
    }
    query_obj.start();
    if let Err(e) = query_obj.initialize().await {
        if let Some(materialized) = materialized.as_ref() {
            materialized.cleanup().await;
        }
        return Err(e);
    }

    // Spawn a detached write task that drains the caller's stream and writes
    // each message as a JSON line. Stdin is closed after the stream ends so
    // the CLI can detect EOF and emit its result message.
    let writer = query_obj.writer_arc();
    let needs_stdin = query_obj.needs_stdin_for_control();
    let write_task = tokio::spawn(async move {
        use futures::StreamExt;
        futures::pin_mut!(messages);
        while let Some(msg) = messages.next().await {
            let payload = format!("{msg}\n");
            if writer.lock().await.write(&payload).await.is_err() {
                break;
            }
        }
        // For SDK MCP servers and hooks the CLI uses stdin for bidirectional
        // control-protocol messages. The read loop (via the result message)
        // is the right moment to close — but since we have no first-result
        // event here, we only close immediately when there is no control
        // traffic expected. When control traffic is present the writer Drop
        // will close stdin on QueryHandle::close().
        if !needs_stdin {
            let _ = writer.lock().await.end_input().await;
        }
    });

    let rx = query_obj
        .take_receiver()
        .ok_or_else(|| ClaudeSdkError::new("message receiver already taken"))?;

    Ok(QueryHandle {
        query: Some(query_obj),
        rx,
        materialized,
        write_task: Some(write_task),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{CanUseToolCallback, PermissionResult};
    use std::sync::Arc;

    #[tokio::test]
    async fn query_rejects_can_use_tool_with_string_prompt() {
        let cb: CanUseToolCallback = Arc::new(|_, _, _| {
            Box::pin(async {
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
        });
        let config = QueryConfig {
            can_use_tool: Some(cb),
            ..Default::default()
        };
        let result = query_with_config("hello", ClaudeAgentOptions::default(), config).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.message.contains("streaming mode"));
    }

    #[tokio::test]
    async fn query_rejects_can_use_tool_with_permission_prompt_tool() {
        let cb: CanUseToolCallback = Arc::new(|_, _, _| {
            Box::pin(async {
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
        });
        let config = QueryConfig {
            can_use_tool: Some(cb),
            ..Default::default()
        };
        let opts = ClaudeAgentOptions {
            permission_prompt_tool_name: Some("CustomTool".into()),
            ..Default::default()
        };
        let result = query_with_config("hi", opts, config).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.message.contains("streaming mode"));
    }
}
