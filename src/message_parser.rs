//! Message parser for Claude Code SDK responses.
//!
//! 1:1 port of `claude_agent_sdk._internal.message_parser.parse_message`.
//! Consumes a raw JSON value (the deserialized CLI line) and produces a
//! typed [`Message`], `Ok(None)` for forward-compatible unknown types, or a
//! [`MessageParseError`] for malformed payloads.

use serde_json::{Map, Value};

use crate::error::MessageParseError;
use crate::types::*;

// ---- value access helpers (mirror Python dict access / KeyError messages) ----

fn require_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    ctx: &str,
) -> Result<&'a str, MessageParseError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(other) => Err(MessageParseError::new(
            format!(
                "{ctx}: {key} (expected string, got {})",
                json_type_name(other)
            ),
            None,
        )),
        None => Err(MessageParseError::new(format!("{ctx}: {key}"), None)),
    }
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn get_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    match obj.get(key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn get_bool(obj: &Map<String, Value>, key: &str) -> Option<bool> {
    obj.get(key).and_then(Value::as_bool)
}

fn get_i64(obj: &Map<String, Value>, key: &str) -> Option<i64> {
    obj.get(key).and_then(Value::as_i64)
}

// ---- content block parsing ----

fn parse_tool_result_content(block: &Map<String, Value>) -> Option<Value> {
    match block.get("content") {
        Some(Value::Null) | None => None,
        Some(v) => Some(v.clone()),
    }
}

/// Parse a single content block; mirrors the per-block `match block["type"]`.
///
/// Returns `Ok(None)` for unknown block types so they are silently skipped,
/// matching the Python parser's `match` statement (which has no `case _`
/// default, so unrecognized blocks like the newer `fallback` type are
/// dropped while the rest of the message still parses).
///
/// Propagates missing required fields as a "Missing required field in {ctx} message: {key}".
fn parse_content_block(
    block: &Map<String, Value>,
    ctx: &str,
) -> Result<Option<ContentBlock>, MessageParseError> {
    let btype = match block.get("type") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(MessageParseError::new(format!("{ctx}: type"), None)),
    };

    match btype {
        "text" => {
            let text = require_str(block, "text", ctx)?;
            Ok(Some(ContentBlock::Text(TextBlock {
                text: text.to_string(),
            })))
        }
        "thinking" => {
            let thinking = require_str(block, "thinking", ctx)?;
            let signature = require_str(block, "signature", ctx)?;
            Ok(Some(ContentBlock::Thinking(ThinkingBlock {
                thinking: thinking.to_string(),
                signature: signature.to_string(),
            })))
        }
        "tool_use" => {
            let id = require_str(block, "id", ctx)?;
            let name = require_str(block, "name", ctx)?;
            let input = require_object_field(block, "input", ctx)?;
            Ok(Some(ContentBlock::ToolUse(ToolUseBlock {
                id: id.to_string(),
                name: name.to_string(),
                input,
            })))
        }
        "tool_result" => {
            let tool_use_id = require_str(block, "tool_use_id", ctx)?;
            Ok(Some(ContentBlock::ToolResult(ToolResultBlock {
                tool_use_id: tool_use_id.to_string(),
                content: parse_tool_result_content(block),
                is_error: get_bool(block, "is_error"),
            })))
        }
        "server_tool_use" => {
            let id = require_str(block, "id", ctx)?;
            let name = require_str(block, "name", ctx)?;
            let input = require_object_field(block, "input", ctx)?;
            let server_name = ServerToolName::from_str_lossy(name).ok_or_else(|| {
                MessageParseError::new(format!("{ctx}: name (unknown server tool '{name}')"), None)
            })?;
            Ok(Some(ContentBlock::ServerToolUse(ServerToolUseBlock {
                id: id.to_string(),
                name: server_name,
                input,
            })))
        }
        "advisor_tool_result" => {
            let tool_use_id = require_str(block, "tool_use_id", ctx)?;
            let content = require_object_field(block, "content", ctx)?;
            Ok(Some(ContentBlock::ServerToolResult(
                ServerToolResultBlock {
                    tool_use_id: tool_use_id.to_string(),
                    content,
                },
            )))
        }
        // Unknown block types are silently skipped (forward compatibility),
        // matching the Python parser's match-without-default behavior.
        _ => Ok(None),
    }
}

fn require_object_field(
    obj: &Map<String, Value>,
    key: &str,
    ctx: &str,
) -> Result<Map<String, Value>, MessageParseError> {
    match obj.get(key) {
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(Value::Null) | None => Err(MessageParseError::new(format!("{ctx}: {key}"), None)),
        Some(other) => Err(MessageParseError::new(
            format!(
                "{ctx}: {key} (expected object, got {})",
                json_type_name(other)
            ),
            None,
        )),
    }
}

// ---- user message parsing ----

fn parse_user_message(data: &Map<String, Value>) -> Result<UserMessage, MessageParseError> {
    let ctx = "Missing required field in user message";
    let message = data
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: message"), None))?;
    let content_val = message
        .get("content")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: content"), None))?;

    let content = match content_val {
        Value::Array(items) => {
            let mut blocks = Vec::with_capacity(items.len());
            for item in items {
                let block = item.as_object().ok_or_else(|| {
                    MessageParseError::new(format!("{ctx}: content (block not object)"), None)
                })?;
                if let Some(b) = parse_content_block(block, ctx)? {
                    blocks.push(b);
                }
            }
            UserContent::Blocks(blocks)
        }
        Value::String(s) => UserContent::Text(s.clone()),
        other => {
            // Python stores whatever is present as content; mirror by stringifying
            // non-list/non-string into the closest faithful form. The Python
            // branch for non-list content stores the raw value; since our
            // UserContent is Text | Blocks, we coerce numbers/bools to their
            // JSON string form to avoid dropping data.
            UserContent::Text(other.to_string())
        }
    };

    Ok(UserMessage {
        content,
        uuid: get_str(data, "uuid").map(String::from),
        parent_tool_use_id: get_str(data, "parent_tool_use_id").map(String::from),
        tool_use_result: match data.get("tool_use_result") {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.clone()),
        },
    })
}

// ---- assistant message parsing ----

fn parse_assistant_message(
    data: &Map<String, Value>,
) -> Result<AssistantMessage, MessageParseError> {
    let ctx = "Missing required field in assistant message";
    let message = data
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: message"), None))?;
    let model = require_str(message, "model", ctx)?;
    let content_arr = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: content"), None))?;

    let mut blocks = Vec::with_capacity(content_arr.len());
    for item in content_arr {
        let block = item.as_object().ok_or_else(|| {
            MessageParseError::new(format!("{ctx}: content (block not object)"), None)
        })?;
        if let Some(b) = parse_content_block(block, ctx)? {
            blocks.push(b);
        }
    }

    let error = match data.get("error") {
        Some(Value::String(s)) => AssistantMessageError::from_str_lossy(s),
        _ => None,
    };

    Ok(AssistantMessage {
        content: blocks,
        model: model.to_string(),
        parent_tool_use_id: get_str(data, "parent_tool_use_id").map(String::from),
        error,
        usage: match message.get("usage") {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.clone()),
        },
        message_id: get_str(message, "id").map(String::from),
        stop_reason: get_str(message, "stop_reason").map(String::from),
        session_id: get_str(data, "session_id").map(String::from),
        uuid: get_str(data, "uuid").map(String::from),
    })
}

// ---- system message parsing ----

fn parse_system_message(data: &Map<String, Value>) -> Result<Message, MessageParseError> {
    let ctx = "Missing required field in system message";
    let subtype = require_str(data, "subtype", ctx)?;
    let data_value = Value::Object(data.clone());

    match subtype {
        "task_started" => Ok(Message::TaskStarted(TaskStartedMessage {
            subtype: subtype.to_string(),
            data: data_value,
            task_id: require_str(data, "task_id", ctx)?.to_string(),
            description: require_str(data, "description", ctx)?.to_string(),
            uuid: require_str(data, "uuid", ctx)?.to_string(),
            session_id: require_str(data, "session_id", ctx)?.to_string(),
            tool_use_id: get_str(data, "tool_use_id").map(String::from),
            task_type: get_str(data, "task_type").map(String::from),
        })),
        "task_progress" => {
            let usage_val = data
                .get("usage")
                .ok_or_else(|| MessageParseError::new(format!("{ctx}: usage"), None))?;
            let usage = TaskUsage::from_value(usage_val).ok_or_else(|| {
                MessageParseError::new(format!("{ctx}: usage (invalid shape)"), None)
            })?;
            Ok(Message::TaskProgress(TaskProgressMessage {
                subtype: subtype.to_string(),
                data: data_value,
                task_id: require_str(data, "task_id", ctx)?.to_string(),
                description: require_str(data, "description", ctx)?.to_string(),
                usage,
                uuid: require_str(data, "uuid", ctx)?.to_string(),
                session_id: require_str(data, "session_id", ctx)?.to_string(),
                tool_use_id: get_str(data, "tool_use_id").map(String::from),
                last_tool_name: get_str(data, "last_tool_name").map(String::from),
            }))
        }
        "task_notification" => {
            let status_str = require_str(data, "status", ctx)?;
            let status = TaskNotificationStatus::from_str_lossy(status_str).ok_or_else(|| {
                MessageParseError::new(format!("{ctx}: status (invalid '{status_str}')"), None)
            })?;
            let usage = match data.get("usage") {
                Some(Value::Null) | None => None,
                Some(v) => TaskUsage::from_value(v),
            };
            Ok(Message::TaskNotification(TaskNotificationMessage {
                subtype: subtype.to_string(),
                data: data_value,
                task_id: require_str(data, "task_id", ctx)?.to_string(),
                status,
                output_file: require_str(data, "output_file", ctx)?.to_string(),
                summary: require_str(data, "summary", ctx)?.to_string(),
                uuid: require_str(data, "uuid", ctx)?.to_string(),
                session_id: require_str(data, "session_id", ctx)?.to_string(),
                tool_use_id: get_str(data, "tool_use_id").map(String::from),
                usage,
            }))
        }
        "task_updated" => {
            let patch = match data.get("patch") {
                Some(Value::Object(m)) => m.clone(),
                _ => Map::new(),
            };
            let status = patch
                .get("status")
                .and_then(Value::as_str)
                .and_then(TaskUpdatedStatus::from_str_lossy);
            Ok(Message::TaskUpdated(TaskUpdatedMessage {
                subtype: subtype.to_string(),
                data: data_value,
                task_id: get_str(data, "task_id").unwrap_or("").to_string(),
                patch,
                status,
                session_id: get_str(data, "session_id").map(String::from),
                uuid: get_str(data, "uuid").map(String::from),
            }))
        }
        "mirror_error" => Ok(Message::MirrorError(MirrorErrorMessage {
            subtype: subtype.to_string(),
            data: data_value,
            key: data.get("key").filter(|v| !v.is_null()).cloned(),
            error: get_str(data, "error").unwrap_or("").to_string(),
        })),
        _ => Ok(Message::System(SystemMessage {
            subtype: subtype.to_string(),
            data: data_value,
        })),
    }
}

// ---- result message parsing ----

fn parse_result_message(data: &Map<String, Value>) -> Result<ResultMessage, MessageParseError> {
    let ctx = "Missing required field in result message";
    let subtype = require_str(data, "subtype", ctx)?.to_string();
    let duration_ms = get_i64(data, "duration_ms")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: duration_ms"), None))?;
    let duration_api_ms = get_i64(data, "duration_api_ms")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: duration_api_ms"), None))?;
    let is_error = get_bool(data, "is_error")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: is_error"), None))?;
    let num_turns = get_i64(data, "num_turns")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: num_turns"), None))?;
    let session_id = require_str(data, "session_id", ctx)?.to_string();

    let deferred_tool_use = match data.get("deferred_tool_use") {
        Some(Value::Null) | None => None,
        Some(v) => {
            let obj = v
                .as_object()
                .ok_or_else(|| MessageParseError::new(format!("{ctx}: deferred_tool_use"), None))?;
            let id = require_str(obj, "id", ctx)?.to_string();
            let name = require_str(obj, "name", ctx)?.to_string();
            let input = require_object_field(obj, "input", ctx)?;
            Some(DeferredToolUse { id, name, input })
        }
    };

    Ok(ResultMessage {
        subtype,
        duration_ms,
        duration_api_ms,
        is_error,
        num_turns,
        session_id,
        stop_reason: get_str(data, "stop_reason").map(String::from),
        total_cost_usd: data.get("total_cost_usd").and_then(Value::as_f64),
        usage: match data.get("usage") {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.clone()),
        },
        result: get_str(data, "result").map(String::from),
        structured_output: match data.get("structured_output") {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.clone()),
        },
        model_usage: match data.get("modelUsage") {
            Some(Value::Null) | None => None,
            Some(v) => Some(v.clone()),
        },
        permission_denials: match data.get("permission_denials") {
            Some(Value::Null) | None => None,
            Some(v) => v.as_array().cloned(),
        },
        deferred_tool_use,
        errors: match data.get("errors") {
            Some(Value::Null) | None => None,
            Some(Value::Array(arr)) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    match item.as_str() {
                        Some(s) => out.push(s.to_string()),
                        None => {
                            return Err(MessageParseError::new(
                                format!("{ctx}: errors (non-string entry)"),
                                None,
                            ))
                        }
                    }
                }
                Some(out)
            }
            Some(_) => None,
        },
        api_error_status: get_i64(data, "api_error_status"),
        uuid: get_str(data, "uuid").map(String::from),
    })
}

// ---- stream event parsing ----

fn parse_stream_event(data: &Map<String, Value>) -> Result<StreamEvent, MessageParseError> {
    let ctx = "Missing required field in stream_event message";
    let uuid = require_str(data, "uuid", ctx)?.to_string();
    let session_id = require_str(data, "session_id", ctx)?.to_string();
    let event = data
        .get("event")
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: event"), None))?
        .clone();
    Ok(StreamEvent {
        uuid,
        session_id,
        event,
        parent_tool_use_id: get_str(data, "parent_tool_use_id").map(String::from),
    })
}

// ---- rate limit event parsing ----

fn parse_rate_limit_event(data: &Map<String, Value>) -> Result<RateLimitEvent, MessageParseError> {
    let ctx = "Missing required field in rate_limit_event message";
    let info = data
        .get("rate_limit_info")
        .and_then(Value::as_object)
        .ok_or_else(|| MessageParseError::new(format!("{ctx}: rate_limit_info"), None))?;

    let status_str = require_str(info, "status", ctx)?;
    let status = RateLimitStatus::from_str_lossy(status_str).ok_or_else(|| {
        MessageParseError::new(format!("{ctx}: status (invalid '{status_str}')"), None)
    })?;

    let rate_limit_type = get_str(info, "rateLimitType").and_then(RateLimitType::from_str_lossy);
    let overage_status = get_str(info, "overageStatus").and_then(RateLimitStatus::from_str_lossy);

    let rate_limit_info = RateLimitInfo {
        status,
        resets_at: get_i64(info, "resetsAt"),
        rate_limit_type,
        utilization: info.get("utilization").and_then(Value::as_f64),
        overage_status,
        overage_resets_at: get_i64(info, "overageResetsAt"),
        overage_disabled_reason: get_str(info, "overageDisabledReason").map(String::from),
        raw: Value::Object(info.clone()),
    };

    Ok(RateLimitEvent {
        rate_limit_info,
        uuid: require_str(data, "uuid", ctx)?.to_string(),
        session_id: require_str(data, "session_id", ctx)?.to_string(),
    })
}

// ---- hook event parsing ----

fn parse_hook_event_message(data: &Map<String, Value>, subtype: &str) -> HookEventMessage {
    let hook_event_name = get_str(data, "hook_event")
        .or_else(|| get_str(data, "hook_name"))
        .or_else(|| get_str(data, "hook_event_name"))
        .unwrap_or("")
        .to_string();
    HookEventMessage {
        subtype: subtype.to_string(),
        data: Value::Object(data.clone()),
        hook_event_name,
        session_id: get_str(data, "session_id").map(String::from),
        uuid: get_str(data, "uuid").map(String::from),
    }
}

// ---- top-level parse_message ----

/// Parse a raw CLI message value into a typed [`Message`].
///
/// Returns:
/// - `Ok(Some(message))` for recognized message types.
/// - `Ok(None)` for unrecognized message types (forward compatibility — newer
///   CLI versions don't crash older SDK versions).
/// - `Err(MessageParseError)` for malformed payloads (missing required fields,
///   invalid data type, etc.).
pub fn parse_message(data: &Value) -> Result<Option<Message>, MessageParseError> {
    let obj = match data {
        Value::Object(m) => m,
        _ => {
            return Err(MessageParseError::new(
                format!(
                    "Invalid message data type (expected dict, got {})",
                    json_type_name(data)
                ),
                Some(data.clone()),
            ))
        }
    };

    // Hook events: system messages with hook_started/hook_response subtype.
    if get_str(obj, "type") == Some("system")
        && matches!(
            get_str(obj, "subtype"),
            Some("hook_started") | Some("hook_response")
        )
    {
        let subtype = get_str(obj, "subtype").unwrap();
        return Ok(Some(Message::HookEvent(parse_hook_event_message(
            obj, subtype,
        ))));
    }

    let message_type = match obj.get("type") {
        Some(Value::String(s)) => s.as_str(),
        Some(_) | None => {
            return Err(MessageParseError::new(
                "Message missing 'type' field",
                Some(data.clone()),
            ))
        }
    };

    // Per-type parsers report errors with `data = None`; the dispatcher
    // attaches the original payload so callers see the same dict the Python
    // `MessageParseError.data` exposes (see test_message_parse_error_contains_data).
    let result: Result<Option<Message>, MessageParseError> = match message_type {
        "user" => parse_user_message(obj).map(Message::User).map(Some),
        "assistant" => parse_assistant_message(obj)
            .map(Message::Assistant)
            .map(Some),
        "system" => parse_system_message(obj).map(Some),
        "result" => parse_result_message(obj).map(Message::Result).map(Some),
        "stream_event" => parse_stream_event(obj).map(Message::StreamEvent).map(Some),
        "rate_limit_event" => parse_rate_limit_event(obj)
            .map(Message::RateLimitEvent)
            .map(Some),
        // Forward-compatible: skip unrecognized message types.
        _ => Ok(None),
    };
    result.map_err(|mut e| {
        if e.data.is_none() {
            e.data = Some(data.clone());
        }
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn parses_unknown_type_as_none() {
        let m = parse_message(&json(r#"{"type":"unknown_type"}"#)).unwrap();
        assert!(m.is_none());
    }

    #[test]
    fn rejects_non_object() {
        let err = parse_message(&json(r#""not a dict""#)).unwrap_err();
        assert!(err.message.contains("Invalid message data type"));
        assert!(err.message.contains("expected dict, got string"));
    }

    #[test]
    fn rejects_missing_type_field() {
        let err = parse_message(&json(r#"{"message":{"content":[]}}"#)).unwrap_err();
        assert!(err.message.contains("Message missing 'type' field"));
    }
}
