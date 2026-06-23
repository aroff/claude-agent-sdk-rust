//! In-process SDK MCP tool builders.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::control::McpServerHandler;
use crate::options::McpServerConfig;

pub type SdkMcpToolFuture = Pin<Box<dyn Future<Output = Value> + Send>>;
pub type SdkMcpToolHandler = Arc<dyn Fn(Value) -> SdkMcpToolFuture + Send + Sync>;

#[derive(Clone)]
pub struct SdkMcpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub handler: SdkMcpToolHandler,
    pub annotations: Option<Value>,
}

impl SdkMcpTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: F,
    ) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Value> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            handler: Arc::new(move |args| Box::pin(handler(args))),
            annotations: None,
        }
    }

    pub fn with_annotations(mut self, annotations: Value) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

pub fn tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    handler: F,
) -> SdkMcpTool
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Value> + Send + 'static,
{
    SdkMcpTool::new(name, description, input_schema, handler)
}

#[derive(Clone)]
pub struct SdkMcpServer {
    pub name: String,
    pub version: String,
    pub tools: BTreeMap<String, SdkMcpTool>,
}

#[derive(Clone)]
pub struct SdkMcpServerConfig {
    pub config: McpServerConfig,
    pub handler: McpServerHandler,
}

pub fn create_sdk_mcp_server(
    name: impl Into<String>,
    version: impl Into<String>,
    tools: Vec<SdkMcpTool>,
) -> SdkMcpServerConfig {
    let server = Arc::new(SdkMcpServer {
        name: name.into(),
        version: version.into(),
        tools: tools.into_iter().map(|t| (t.name.clone(), t)).collect(),
    });

    let mut config = Map::new();
    config.insert("type".into(), Value::String("sdk".into()));
    config.insert("name".into(), Value::String(server.name.clone()));
    config.insert("version".into(), Value::String(server.version.clone()));

    let handler_server = server.clone();
    let handler: McpServerHandler = Arc::new(move |message| {
        let server = handler_server.clone();
        Box::pin(async move { handle_jsonrpc(server, message).await })
    });

    SdkMcpServerConfig { config, handler }
}

async fn handle_jsonrpc(server: Arc<SdkMcpServer>, message: Value) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "initialize" => response(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {"name": server.name, "version": server.version},
                "capabilities": {"tools": {}}
            }),
        ),
        "tools/list" => response(
            id,
            json!({
                "tools": server.tools.values().map(tool_to_json).collect::<Vec<_>>()
            }),
        ),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match server.tools.get(name) {
                Some(tool) => {
                    let result = (tool.handler)(args).await;
                    response(id, normalize_tool_result(result))
                }
                None => error(id, -32601, format!("Tool '{name}' not found")),
            }
        }
        "notifications/initialized" => Value::Null,
        _ => error(id, -32601, format!("Method '{method}' not found")),
    }
}

fn tool_to_json(tool: &SdkMcpTool) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(tool.name.clone()));
    obj.insert(
        "description".into(),
        Value::String(tool.description.clone()),
    );
    obj.insert("inputSchema".into(), tool.input_schema.clone());
    if let Some(annotations) = &tool.annotations {
        obj.insert("annotations".into(), annotations.clone());
    }
    Value::Object(obj)
}

fn normalize_tool_result(result: Value) -> Value {
    if result.get("content").is_some() {
        result
    } else {
        json!({"content": [{"type": "text", "text": result.to_string()}]})
    }
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sdk_mcp_lists_and_calls_tools() {
        let add = tool(
            "add",
            "Add numbers",
            json!({"type":"object","properties":{"a":{"type":"number"},"b":{"type":"number"}},"required":["a","b"]}),
            |args| async move {
                let a = args.get("a").and_then(Value::as_f64).unwrap_or(0.0);
                let b = args.get("b").and_then(Value::as_f64).unwrap_or(0.0);
                json!({"content":[{"type":"text","text":(a + b).to_string()}]})
            },
        );
        let server = create_sdk_mcp_server("calc", "1.0.0", vec![add]);
        assert_eq!(
            server.config.get("type").and_then(Value::as_str),
            Some("sdk")
        );
        let list = (server.handler)(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
        assert_eq!(list["result"]["tools"][0]["name"], "add");
        let call = (server.handler)(json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"add","arguments":{"a":2,"b":3}}
        }))
        .await;
        assert_eq!(call["result"]["content"][0]["text"], "5");
    }
}
