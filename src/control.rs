//! Control protocol: bidirectional control request/response routing on top
//! of a [`Transport`].
//!
//! Mirrors the Python `Query` class. Handles:
//! - the `initialize` handshake (sending agents / skills / hooks config)
//! - outgoing control requests (interrupt, set_permission_mode, etc.)
//! - incoming control requests from the CLI (can_use_tool, hook_callback,
//!   mcp_message) routed to caller-supplied callbacks
//! - the message stream (regular SDK messages forwarded to consumers)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
#[allow(unused_imports)]
use tracing::debug;

use crate::error::ClaudeSdkError;
use crate::session::{SessionKey, TranscriptMirrorBatcher};
use crate::transport::{Transport, TransportReader, TransportWriter};
use crate::types::PermissionMode;

/// Identifier for a pending control request awaiting a CLI response.
pub type RequestId = String;

/// Outcome of a control request: either the CLI's response payload or an
/// error (timeout, CLI-reported error, transport failure).
pub type ControlResult = Result<Value, ClaudeSdkError>;

/// Hook callback signature: `async (input, tool_use_id) -> hook_output`.
pub type HookCallback = Arc<
    dyn Fn(
            Value,
            Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>
        + Send
        + Sync,
>;

/// Tool-permission callback signature.
pub type CanUseToolCallback = Arc<
    dyn Fn(
            String,
            Value,
            ToolPermissionContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PermissionResult> + Send>>
        + Send
        + Sync,
>;

/// Context passed to a `can_use_tool` callback.
#[derive(Debug, Clone, Default)]
pub struct ToolPermissionContext {
    pub suggestions: Vec<crate::types::PermissionUpdate>,
    pub tool_use_id: Option<String>,
    pub agent_id: Option<String>,
    pub blocked_path: Option<String>,
    pub decision_reason: Option<String>,
    pub title: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

/// Result of a `can_use_tool` decision.
#[derive(Debug, Clone)]
pub enum PermissionResult {
    Allow {
        updated_input: Option<Value>,
        updated_permissions: Option<Vec<crate::types::PermissionUpdate>>,
    },
    Deny {
        message: String,
        interrupt: bool,
    },
}

/// Hook matcher configuration sent during the initialize handshake.
#[derive(Clone)]
pub struct HookMatcherConfig {
    pub matcher: Option<String>,
    pub hooks: Vec<HookCallback>,
    pub timeout: Option<Duration>,
}

/// Configuration for the [`Query`] control-protocol layer.
#[derive(Clone)]
pub struct QueryConfig {
    pub can_use_tool: Option<CanUseToolCallback>,
    pub hooks: HashMap<String, Vec<HookMatcherConfig>>,
    pub initialize_timeout: Duration,
    pub agents: Option<Map<String, Value>>,
    pub exclude_dynamic_sections: Option<bool>,
    pub skills: Option<Vec<String>>,
    /// SDK MCP server handlers keyed by server name. Each handler receives
    /// the raw JSONRPC message and returns a JSONRPC response.
    pub sdk_mcp_servers: HashMap<String, McpServerHandler>,
    /// Filled in by `initialize()`; available via `initialization_result()`.
    pub initialization_result: Option<Value>,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            can_use_tool: None,
            hooks: HashMap::new(),
            initialize_timeout: Duration::from_secs(60),
            agents: None,
            exclude_dynamic_sections: None,
            skills: None,
            sdk_mcp_servers: HashMap::new(),
            initialization_result: None,
        }
    }
}

impl QueryConfig {
    pub fn with_sdk_mcp_server(
        mut self,
        server_name: impl Into<String>,
        server: crate::sdk_mcp::SdkMcpServerConfig,
    ) -> Self {
        self.sdk_mcp_servers
            .insert(server_name.into(), server.handler);
        self
    }
}

/// Handler for an SDK MCP server: receives a JSONRPC message, returns a
/// JSONRPC response.
pub type McpServerHandler = Arc<
    dyn Fn(Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>
        + Send
        + Sync,
>;

/// Query manages the bidirectional control protocol over a [`Transport`].
///
/// Created by [`crate::query::query`]; not intended to be constructed
/// directly by application code.
pub struct Query {
    reader: Option<Box<dyn TransportReader>>,
    writer: Arc<Mutex<Box<dyn TransportWriter>>>,
    config: QueryConfig,
    msg_tx: mpsc::Sender<Result<Value, ClaudeSdkError>>,
    msg_rx: Option<mpsc::Receiver<Result<Value, ClaudeSdkError>>>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<ControlResult>>>>,
    hook_callbacks: Arc<Mutex<HashMap<String, HookCallback>>>,
    request_counter: Arc<std::sync::atomic::AtomicU64>,
    hook_counter: Arc<std::sync::atomic::AtomicU64>,
    read_handle: Option<tokio::task::JoinHandle<()>>,
    ended_input: Arc<std::sync::atomic::AtomicBool>,
    transcript_mirror_batcher: Option<Arc<TranscriptMirrorBatcher>>,
}

impl Query {
    /// Construct a Query over a connected transport. Splits the transport
    /// into reader/writer halves immediately.
    pub fn new(transport: Box<dyn Transport>, config: QueryConfig) -> Result<Self, ClaudeSdkError> {
        let (reader, writer) = transport.split()?;
        let (msg_tx, msg_rx) = mpsc::channel(100);
        Ok(Self {
            reader: Some(reader),
            writer: Arc::new(Mutex::new(writer)),
            config,
            msg_tx,
            msg_rx: Some(msg_rx),
            pending: Arc::new(Mutex::new(HashMap::new())),
            hook_callbacks: Arc::new(Mutex::new(HashMap::new())),
            request_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            hook_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            read_handle: None,
            ended_input: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            transcript_mirror_batcher: None,
        })
    }

    /// Attach a transcript mirror batcher for `transcript_mirror` frames.
    pub fn set_transcript_mirror_batcher(&mut self, batcher: TranscriptMirrorBatcher) {
        self.transcript_mirror_batcher = Some(Arc::new(batcher));
    }

    /// Inject a mirror error message into the consumer stream.
    pub fn report_mirror_error(&self, key: SessionKey, error: String) {
        let _ = self
            .msg_tx
            .try_send(Ok(Self::mirror_error_message(key, error)));
    }

    /// Callback suitable for [`build_mirror_batcher`](crate::session::build_mirror_batcher).
    pub fn mirror_error_callback(&self) -> Arc<dyn Fn(SessionKey, String) + Send + Sync> {
        let tx = self.msg_tx.clone();
        Arc::new(move |key, error| {
            let _ = tx.try_send(Ok(Self::mirror_error_message(key, error)));
        })
    }

    /// Flush pending transcript mirror entries, if any.
    pub async fn flush_transcript_mirror(&self) {
        if let Some(batcher) = &self.transcript_mirror_batcher {
            batcher.flush().await;
        }
    }

    fn mirror_error_message(key: SessionKey, error: String) -> Value {
        let msg = json!({
            "type": "system",
            "subtype": "mirror_error",
            "session_id": key.session_id.clone(),
            "uuid": Value::Null,
            "key": {
                "project_key": key.project_key.clone(),
                "session_id": key.session_id.clone(),
                "subpath": key.subpath.clone(),
            },
            "error": error,
        });
        msg
    }

    /// Start the background read loop that routes incoming messages.
    pub fn start(&mut self) {
        let reader = self
            .reader
            .take()
            .expect("reader already taken; start() called twice?");
        let msg_tx = self.msg_tx.clone();
        let pending = self.pending.clone();
        let hook_callbacks = self.hook_callbacks.clone();
        let can_use_tool = self.config.can_use_tool.clone();
        let sdk_mcp_servers = Arc::new(self.config.sdk_mcp_servers.clone());
        let writer = self.writer.clone();
        let transcript_mirror_batcher = self.transcript_mirror_batcher.clone();

        let routing = ReadLoopRouting {
            msg_tx,
            writer,
            pending,
            hook_callbacks,
            can_use_tool,
            sdk_mcp_servers,
            transcript_mirror_batcher,
        };

        self.read_handle = Some(tokio::spawn(async move {
            read_loop(reader, routing).await;
        }));
    }

    /// Run the initialize handshake.
    pub async fn initialize(&mut self) -> Result<Value, ClaudeSdkError> {
        let mut hooks_config: Map<String, Value> = Map::new();
        for (event, matchers) in &self.config.hooks {
            if matchers.is_empty() {
                continue;
            }
            let mut entries = Vec::new();
            for m in matchers {
                let mut callback_ids = Vec::new();
                for cb in &m.hooks {
                    let id = format!(
                        "hook_{}",
                        self.hook_counter
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    );
                    self.hook_callbacks
                        .lock()
                        .await
                        .insert(id.clone(), cb.clone());
                    callback_ids.push(id);
                }
                let mut entry = Map::new();
                entry.insert(
                    "matcher".into(),
                    m.matcher.clone().map(Value::String).unwrap_or(Value::Null),
                );
                entry.insert(
                    "hookCallbackIds".into(),
                    Value::Array(callback_ids.into_iter().map(Value::String).collect()),
                );
                if let Some(t) = m.timeout {
                    entry.insert(
                        "timeout".into(),
                        Value::Number((t.as_millis() as i64).into()),
                    );
                }
                entries.push(Value::Object(entry));
            }
            hooks_config.insert(event.clone(), Value::Array(entries));
        }

        let mut request = Map::new();
        request.insert("subtype".into(), Value::String("initialize".into()));
        request.insert(
            "hooks".into(),
            if hooks_config.is_empty() {
                Value::Null
            } else {
                Value::Object(hooks_config)
            },
        );
        if let Some(agents) = &self.config.agents {
            request.insert("agents".into(), Value::Object(agents.clone()));
        }
        if let Some(eds) = self.config.exclude_dynamic_sections {
            request.insert("excludeDynamicSections".into(), Value::Bool(eds));
        }
        if let Some(skills) = &self.config.skills {
            request.insert(
                "skills".into(),
                Value::Array(skills.iter().cloned().map(Value::String).collect()),
            );
        }

        let result = self
            .send_control_request(Value::Object(request), Some(self.config.initialize_timeout))
            .await?;
        self.config.initialization_result = Some(result.clone());
        Ok(result)
    }

    /// Send an arbitrary control request and await its response.
    pub async fn send_control_request(
        &self,
        request: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, ClaudeSdkError> {
        let id = format!(
            "req_{}_{}",
            self.request_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            uuid::Uuid::new_v4().as_simple()
        );
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        let envelope = json!({
            "type": "control_request",
            "request_id": id,
            "request": request,
        });
        let payload = format!("{}\n", envelope);
        self.writer.lock().await.write(&payload).await?;

        match timeout {
            Some(t) => match tokio::time::timeout(t, rx).await {
                Ok(Ok(r)) => r,
                Ok(Err(_)) => Err(ClaudeSdkError::new("control response channel dropped")),
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    Err(ClaudeSdkError::new(format!(
                        "Control request timeout: {}",
                        request
                            .get("subtype")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                    )))
                }
            },
            None => match rx.await {
                Ok(r) => r,
                Err(_) => Err(ClaudeSdkError::new("control response channel dropped")),
            },
        }
    }

    /// Send the `interrupt` control request.
    pub async fn interrupt(&self) -> Result<(), ClaudeSdkError> {
        self.send_control_request(json!({"subtype": "interrupt"}), None)
            .await
            .map(|_| ())
    }

    /// Change the permission mode.
    pub async fn set_permission_mode(&self, mode: PermissionMode) -> Result<(), ClaudeSdkError> {
        let mode_str = match mode {
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Plan => "plan",
            PermissionMode::BypassPermissions => "bypassPermissions",
            PermissionMode::DontAsk => "dontAsk",
            PermissionMode::Auto => "auto",
        };
        self.send_control_request(
            json!({"subtype": "set_permission_mode", "mode": mode_str}),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Take the consumer-facing message receiver. Returns `None` if already taken.
    pub fn take_receiver(&mut self) -> Option<mpsc::Receiver<Result<Value, ClaudeSdkError>>> {
        self.msg_rx.take()
    }

    /// Write a user message directly to the transport (for string-prompt mode).
    pub async fn write_user_message(&self, prompt: &str) -> Result<(), ClaudeSdkError> {
        let msg = json!({
            "type": "user",
            "session_id": "",
            "message": {"role": "user", "content": prompt},
            "parent_tool_use_id": Value::Null,
        });
        let payload = format!("{}\n", msg);
        self.writer.lock().await.write(&payload).await
    }

    /// Write a raw JSON payload to the transport (for arbitrary message types).
    pub async fn write_raw(&self, payload: &str) -> Result<(), ClaudeSdkError> {
        self.writer.lock().await.write(payload).await
    }

    /// Close stdin (send EOF). For one-shot prompts, call this after
    /// [`write_user_message`] so the CLI processes and emits its result.
    pub async fn end_input(&self) -> Result<(), ClaudeSdkError> {
        self.ended_input
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.writer.lock().await.end_input().await
    }

    /// Change the model mid-conversation.
    pub async fn set_model(&self, model: Option<&str>) -> Result<(), ClaudeSdkError> {
        let request = match model {
            Some(m) => json!({"subtype": "set_model", "model": m}),
            None => json!({"subtype": "set_model", "model": Value::Null}),
        };
        self.send_control_request(request, None).await.map(|_| ())
    }

    /// Rewind tracked files to their state at a specific user message.
    /// Requires `enable_file_checkpointing` to be set.
    pub async fn rewind_files(&self, user_message_id: &str) -> Result<(), ClaudeSdkError> {
        self.send_control_request(
            json!({"subtype": "rewind_files", "user_message_id": user_message_id}),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Reconnect a disconnected or failed MCP server.
    pub async fn reconnect_mcp_server(&self, server_name: &str) -> Result<(), ClaudeSdkError> {
        self.send_control_request(
            json!({"subtype": "mcp_reconnect", "serverName": server_name}),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Enable or disable an MCP server.
    pub async fn toggle_mcp_server(
        &self,
        server_name: &str,
        enabled: bool,
    ) -> Result<(), ClaudeSdkError> {
        self.send_control_request(
            json!({"subtype": "mcp_toggle", "serverName": server_name, "enabled": enabled}),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Stop a running task. A `task_notification` with status `stopped`
    /// follows in the message stream.
    pub async fn stop_task(&self, task_id: &str) -> Result<(), ClaudeSdkError> {
        self.send_control_request(json!({"subtype": "stop_task", "task_id": task_id}), None)
            .await
            .map(|_| ())
    }

    /// Get current MCP server connection status.
    pub async fn get_mcp_status(&self) -> Result<Value, ClaudeSdkError> {
        self.send_control_request(json!({"subtype": "mcp_status"}), None)
            .await
    }

    /// Get a breakdown of current context window usage by category.
    pub async fn get_context_usage(&self) -> Result<Value, ClaudeSdkError> {
        self.send_control_request(json!({"subtype": "get_context_usage"}), None)
            .await
    }

    /// The initialization result from the [`initialize`] handshake, if completed.
    /// Contains supported commands, agents, output styles, etc.
    pub fn initialization_result(&self) -> Option<&Value> {
        self.config.initialization_result.as_ref()
    }

    /// Close the query: abort the read loop. The writer's Drop handles
    /// stdin close and child reaping.
    pub async fn close(mut self) -> Result<(), ClaudeSdkError> {
        if let Some(batcher) = &self.transcript_mirror_batcher {
            batcher.close().await;
        }
        if let Some(handle) = self.read_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        Ok(())
    }
}

/// The background read loop. Routes control messages to pending requests or
/// to the appropriate callback, and forwards regular SDK messages to the
/// consumer stream.
struct ReadLoopRouting {
    msg_tx: mpsc::Sender<Result<Value, ClaudeSdkError>>,
    writer: Arc<Mutex<Box<dyn TransportWriter>>>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<ControlResult>>>>,
    hook_callbacks: Arc<Mutex<HashMap<String, HookCallback>>>,
    can_use_tool: Option<CanUseToolCallback>,
    sdk_mcp_servers: Arc<HashMap<String, McpServerHandler>>,
    transcript_mirror_batcher: Option<Arc<TranscriptMirrorBatcher>>,
}

async fn read_loop(mut reader: Box<dyn TransportReader>, routing: ReadLoopRouting) {
    loop {
        let msg = match reader.read_message().await {
            Ok(Some(m)) => m,
            Ok(None) => {
                if let Some(batcher) = &routing.transcript_mirror_batcher {
                    batcher.flush().await;
                }
                let _ = routing
                    .msg_tx
                    .send(Err(ClaudeSdkError::new("stream ended")))
                    .await;
                break;
            }
            Err(e) => {
                if let Some(batcher) = &routing.transcript_mirror_batcher {
                    batcher.flush().await;
                }
                let _ = routing.msg_tx.send(Err(e)).await;
                break;
            }
        };

        let msg_type = msg
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        match msg_type.as_str() {
            "control_response" => {
                let response = msg.get("response").cloned().unwrap_or(Value::Null);
                let request_id = response
                    .get("request_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let mut pending_guard = routing.pending.lock().await;
                if let Some(tx) = pending_guard.remove(&request_id) {
                    let result = if response.get("subtype").and_then(Value::as_str) == Some("error")
                    {
                        let err_msg = response
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown error")
                            .to_string();
                        Err(ClaudeSdkError::new(err_msg))
                    } else {
                        Ok(response
                            .get("response")
                            .cloned()
                            .unwrap_or(Value::Object(Map::new())))
                    };
                    let _ = tx.send(result);
                }
            }
            "control_request" => {
                let hook_callbacks = routing.hook_callbacks.clone();
                let can_use_tool = routing.can_use_tool.clone();
                let sdk_mcp_servers = routing.sdk_mcp_servers.clone();
                let writer = routing.writer.clone();
                let req = msg.clone();
                tokio::spawn(async move {
                    let resp = handle_incoming_control_request(
                        req,
                        hook_callbacks,
                        can_use_tool,
                        sdk_mcp_servers,
                    )
                    .await;
                    let envelope = json!({
                        "type": "control_response",
                        "response": resp,
                    });
                    let payload = format!("{}\n", envelope);
                    let _ = writer.lock().await.write(&payload).await;
                });
            }
            "transcript_mirror" => {
                if let Some(batcher) = &routing.transcript_mirror_batcher {
                    if let (Some(file_path), Some(entries)) = (
                        msg.get("filePath").and_then(Value::as_str),
                        msg.get("entries").and_then(Value::as_array),
                    ) {
                        batcher
                            .enqueue(file_path.to_string(), entries.clone())
                            .await;
                    }
                }
            }
            _ => {
                if msg_type == "result" {
                    if let Some(batcher) = &routing.transcript_mirror_batcher {
                        batcher.flush().await;
                    }
                }
                if routing.msg_tx.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Handle an incoming `control_request` from the CLI (can_use_tool /
/// hook_callback / mcp_message) and produce the response payload.
async fn handle_incoming_control_request(
    request: Value,
    hook_callbacks: Arc<Mutex<HashMap<String, HookCallback>>>,
    can_use_tool: Option<CanUseToolCallback>,
    sdk_mcp_servers: Arc<HashMap<String, McpServerHandler>>,
) -> Value {
    let request_id = request
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let request_data = request.get("request").cloned().unwrap_or(Value::Null);
    let subtype = request_data
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let (subtype_out, response_data, error) = match subtype.as_str() {
        "can_use_tool" => {
            if let Some(cb) = can_use_tool.as_ref() {
                let tool_name = request_data
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = request_data.get("input").cloned().unwrap_or(Value::Null);
                let ctx = ToolPermissionContext {
                    tool_use_id: request_data
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                    agent_id: request_data
                        .get("agent_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                    blocked_path: request_data
                        .get("blocked_path")
                        .and_then(Value::as_str)
                        .map(String::from),
                    decision_reason: request_data
                        .get("decision_reason")
                        .and_then(Value::as_str)
                        .map(String::from),
                    title: request_data
                        .get("title")
                        .and_then(Value::as_str)
                        .map(String::from),
                    display_name: request_data
                        .get("display_name")
                        .and_then(Value::as_str)
                        .map(String::from),
                    description: request_data
                        .get("description")
                        .and_then(Value::as_str)
                        .map(String::from),
                    suggestions: vec![],
                };
                match cb(tool_name, input, ctx).await {
                    PermissionResult::Allow {
                        updated_input,
                        updated_permissions,
                    } => {
                        let mut rd = Map::new();
                        rd.insert("behavior".into(), Value::String("allow".into()));
                        rd.insert(
                            "updatedInput".into(),
                            updated_input.unwrap_or_else(|| {
                                request_data.get("input").cloned().unwrap_or(Value::Null)
                            }),
                        );
                        if let Some(perms) = updated_permissions {
                            rd.insert(
                                "updatedPermissions".into(),
                                Value::Array(
                                    perms.iter().map(|p| Value::Object(p.to_dict())).collect(),
                                ),
                            );
                        }
                        ("success", Value::Object(rd), None)
                    }
                    PermissionResult::Deny { message, interrupt } => {
                        let mut rd = Map::new();
                        rd.insert("behavior".into(), Value::String("deny".into()));
                        rd.insert("message".into(), Value::String(message));
                        rd.insert("interrupt".into(), Value::Bool(interrupt));
                        ("success", Value::Object(rd), None)
                    }
                }
            } else {
                (
                    "error",
                    Value::Null,
                    Some("canUseTool callback is not provided".to_string()),
                )
            }
        }
        "hook_callback" => {
            let callback_id = request_data
                .get("callback_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let input = request_data.get("input").cloned().unwrap_or(Value::Null);
            let tool_use_id = request_data
                .get("tool_use_id")
                .and_then(Value::as_str)
                .map(String::from);
            let cb_guard = hook_callbacks.lock().await;
            match cb_guard.get(&callback_id) {
                Some(cb) => {
                    let cb = cb.clone();
                    drop(cb_guard);
                    let output = cb(input, tool_use_id).await;
                    ("success", output, None)
                }
                None => (
                    "error",
                    Value::Null,
                    Some(format!("No hook callback found for ID: {callback_id}")),
                ),
            }
        }
        "mcp_message" => {
            let server_name = request_data
                .get("server_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mcp_message = request_data.get("message").cloned().unwrap_or(Value::Null);
            if server_name.is_empty() || mcp_message.is_null() {
                (
                    "error",
                    Value::Null,
                    Some("Missing server_name or message for MCP request".to_string()),
                )
            } else {
                match sdk_mcp_servers.get(&server_name) {
                    Some(handler) => {
                        let handler = handler.clone();
                        let mcp_response = handler(mcp_message).await;
                        let mut rd = Map::new();
                        rd.insert("mcp_response".into(), mcp_response);
                        ("success", Value::Object(rd), None)
                    }
                    None => {
                        let mut error_response = Map::new();
                        error_response.insert("jsonrpc".into(), Value::String("2.0".into()));
                        if let Some(id) = mcp_message.get("id") {
                            error_response.insert("id".into(), id.clone());
                        }
                        let mut err_obj = Map::new();
                        err_obj.insert("code".into(), json!(-32601));
                        err_obj.insert(
                            "message".into(),
                            Value::String(format!("Server '{server_name}' not found")),
                        );
                        error_response.insert("error".into(), Value::Object(err_obj));
                        let mut rd = Map::new();
                        rd.insert("mcp_response".into(), Value::Object(error_response));
                        ("success", Value::Object(rd), None)
                    }
                }
            }
        }
        _ => (
            "error",
            Value::Null,
            Some(format!("Unsupported control request subtype: {subtype}")),
        ),
    };

    let mut response = Map::new();
    response.insert("subtype".into(), Value::String(subtype_out.into()));
    response.insert("request_id".into(), Value::String(request_id));
    if let Some(err) = error {
        response.insert("error".into(), Value::String(err));
    } else {
        response.insert("response".into(), response_data);
    }
    Value::Object(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// A mock transport that splits into a scripted reader and recording writer.
    #[allow(dead_code)]
    struct MockTransport {
        reads: Vec<Option<Value>>,
        writes: Arc<Mutex<Vec<String>>>,
        ready: bool,
    }

    #[allow(dead_code)]
    struct MockReader {
        reads: Vec<Option<Value>>,
    }

    #[async_trait]
    impl TransportReader for MockReader {
        async fn read_message(&mut self) -> Result<Option<Value>, ClaudeSdkError> {
            if self.reads.is_empty() {
                Ok(None)
            } else {
                Ok(self.reads.remove(0))
            }
        }
    }

    #[allow(dead_code)]
    struct MockWriter {
        writes: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl TransportWriter for MockWriter {
        async fn write(&mut self, data: &str) -> Result<(), ClaudeSdkError> {
            self.writes.lock().await.push(data.to_string());
            Ok(())
        }
        async fn end_input(&mut self) -> Result<(), ClaudeSdkError> {
            Ok(())
        }
    }

    #[async_trait]
    impl Transport for MockTransport {
        async fn connect(&mut self) -> Result<(), ClaudeSdkError> {
            self.ready = true;
            Ok(())
        }
        fn split(
            self: Box<Self>,
        ) -> Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>), ClaudeSdkError> {
            Ok((
                Box::new(MockReader { reads: self.reads }),
                Box::new(MockWriter {
                    writes: self.writes,
                }),
            ))
        }
        async fn close(&mut self) -> Result<(), ClaudeSdkError> {
            self.ready = false;
            Ok(())
        }
        fn is_ready(&self) -> bool {
            self.ready
        }
    }

    #[tokio::test]
    async fn control_response_routes_to_pending_request() {
        // Directly exercise the routing logic without the full transport.
        let (tx, rx) = oneshot::channel::<ControlResult>();
        let mut pending: HashMap<RequestId, oneshot::Sender<ControlResult>> = HashMap::new();
        pending.insert("req_1".into(), tx);

        let response = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": "req_1",
                "response": {"ok": true}
            }
        });
        // Simulate what read_loop does for a control_response.
        let request_id = response["response"]["request_id"]
            .as_str()
            .unwrap()
            .to_string();
        if let Some(tx) = pending.remove(&request_id) {
            let result = Ok(response["response"]["response"].clone());
            tx.send(result).unwrap();
        }
        let result = rx.await.unwrap().unwrap();
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn error_response_routes_as_err() {
        let (tx, rx) = oneshot::channel::<ControlResult>();
        let mut pending: HashMap<RequestId, oneshot::Sender<ControlResult>> = HashMap::new();
        pending.insert("req_2".into(), tx);

        let response = json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": "req_2",
                "error": "boom"
            }
        });
        let request_id = response["response"]["request_id"]
            .as_str()
            .unwrap()
            .to_string();
        if let Some(tx) = pending.remove(&request_id) {
            let result = Err(ClaudeSdkError::new(
                response["response"]["error"].as_str().unwrap(),
            ));
            tx.send(result).unwrap();
        }
        let result = rx.await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("boom"));
    }

    #[tokio::test]
    async fn handle_incoming_can_use_tool_allow() {
        let can_use_tool: CanUseToolCallback = Arc::new(|_name, _input, _ctx| {
            Box::pin(async {
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
        });
        let request = json!({
            "request_id": "r1",
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "input": {"command": "ls"},
                "tool_use_id": "tu_1",
            }
        });
        let hook_callbacks = Arc::new(Mutex::new(HashMap::new()));
        let resp = handle_incoming_control_request(
            request,
            hook_callbacks,
            Some(can_use_tool),
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "success");
        assert_eq!(resp["response"]["behavior"], "allow");
        assert_eq!(resp["response"]["updatedInput"]["command"], "ls");
    }

    #[tokio::test]
    async fn handle_incoming_can_use_tool_deny() {
        let can_use_tool: CanUseToolCallback = Arc::new(|_name, _input, _ctx| {
            Box::pin(async {
                PermissionResult::Deny {
                    message: "forbidden".into(),
                    interrupt: false,
                }
            })
        });
        let request = json!({
            "request_id": "r2",
            "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {}}
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            Some(can_use_tool),
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["response"]["behavior"], "deny");
        assert_eq!(resp["response"]["message"], "forbidden");
    }

    #[tokio::test]
    async fn handle_incoming_can_use_tool_without_callback_errors() {
        let request = json!({
            "request_id": "r3",
            "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {}}
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            None,
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "error");
        assert!(resp["error"].as_str().unwrap().contains("canUseTool"));
    }

    #[tokio::test]
    async fn handle_incoming_hook_callback() {
        let cb: HookCallback = Arc::new(|_input, _id| {
            Box::pin(async { json!({"hookEventName": "PreToolUse", "additionalContext": "ctx"}) })
        });
        let mut hooks = HashMap::new();
        hooks.insert("hook_0".to_string(), cb);
        let hook_callbacks = Arc::new(Mutex::new(hooks));
        let request = json!({
            "request_id": "r4",
            "request": {
                "subtype": "hook_callback",
                "callback_id": "hook_0",
                "input": {},
                "tool_use_id": "tu_9",
            }
        });
        let resp = handle_incoming_control_request(
            request,
            hook_callbacks,
            None,
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "success");
        assert_eq!(resp["response"]["additionalContext"], "ctx");
    }

    #[tokio::test]
    async fn handle_incoming_unknown_subtype_errors() {
        let request = json!({
            "request_id": "r5",
            "request": {"subtype": "future_thing"}
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            None,
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "error");
        assert!(resp["error"].as_str().unwrap().contains("future_thing"));
    }

    #[tokio::test]
    async fn handle_incoming_mcp_message_routes_to_handler() {
        let handler: McpServerHandler = Arc::new(|msg| {
            Box::pin(async move {
                json!({
                    "jsonrpc": "2.0",
                    "id": msg.get("id").cloned().unwrap_or(Value::Null),
                    "result": {"tools": []}
                })
            })
        });
        let mut servers = HashMap::new();
        servers.insert("my-server".to_string(), handler);
        let request = json!({
            "request_id": "mcp1",
            "request": {
                "subtype": "mcp_message",
                "server_name": "my-server",
                "message": {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}
            }
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            None,
            Arc::new(servers),
        )
        .await;
        assert_eq!(resp["subtype"], "success");
        assert_eq!(resp["response"]["mcp_response"]["jsonrpc"], "2.0");
        assert_eq!(resp["response"]["mcp_response"]["id"], 1);
    }

    #[tokio::test]
    async fn handle_incoming_mcp_message_unknown_server_returns_jsonrpc_error() {
        let request = json!({
            "request_id": "mcp2",
            "request": {
                "subtype": "mcp_message",
                "server_name": "nonexistent",
                "message": {"jsonrpc": "2.0", "id": 42, "method": "tools/list"}
            }
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            None,
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "success");
        assert_eq!(resp["response"]["mcp_response"]["error"]["code"], -32601);
        assert!(resp["response"]["mcp_response"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nonexistent"));
    }

    #[tokio::test]
    async fn handle_incoming_mcp_message_missing_fields_errors() {
        let request = json!({
            "request_id": "mcp3",
            "request": {"subtype": "mcp_message"}
        });
        let resp = handle_incoming_control_request(
            request,
            Arc::new(Mutex::new(HashMap::new())),
            None,
            Arc::new(HashMap::new()),
        )
        .await;
        assert_eq!(resp["subtype"], "error");
        assert!(resp["error"].as_str().unwrap().contains("Missing"));
    }
}
