//! Integration tests for `parse_message`, mirroring
//! `tests/test_message_parser.py` from the Python SDK.

use claude_agent_sdk::{
    parse_message, AssistantMessageError, Message, ServerToolName, TaskNotificationStatus,
    TaskUpdatedStatus, UserContent,
};
use serde_json::{json, Value};

fn parse(data: Value) -> Message {
    parse_message(&data)
        .expect("expected parse success")
        .expect("expected a message")
}

fn parse_none(data: Value) {
    assert!(parse_message(&data)
        .expect("expected parse success")
        .is_none());
}

fn parse_err(data: Value) -> claude_agent_sdk::MessageParseError {
    parse_message(&data).expect_err("expected parse error")
}

// ---------------------------------------------------------------- user messages

#[test]
fn parse_valid_user_message() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [{"type": "text", "text": "Hello"}]}
    }));
    let u = match m {
        Message::User(u) => u,
        _ => panic!("expected user"),
    };
    let blocks = match &u.content {
        UserContent::Blocks(b) => b,
        _ => panic!("expected blocks"),
    };
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].as_text().unwrap().text, "Hello");
}

#[test]
fn parse_user_message_with_uuid() {
    let m = parse(json!({
        "type": "user",
        "uuid": "msg-abc123-def456",
        "message": {"content": [{"type": "text", "text": "Hello"}]}
    }));
    match m {
        Message::User(u) => {
            assert_eq!(u.uuid.as_deref(), Some("msg-abc123-def456"));
            assert!(matches!(&u.content, UserContent::Blocks(b) if b.len() == 1));
        }
        _ => panic!("expected user"),
    }
}

#[test]
fn parse_user_message_with_tool_use() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [
            {"type": "text", "text": "Let me read this file"},
            {"type": "tool_use", "id": "tool_456", "name": "Read", "input": {"file_path": "/example.txt"}}
        ]}
    }));
    match m {
        Message::User(u) => {
            let blocks = match &u.content {
                UserContent::Blocks(b) => b,
                _ => panic!(),
            };
            assert_eq!(blocks.len(), 2);
            assert!(blocks[0].as_text().is_some());
            let tu = blocks[1].as_tool_use().unwrap();
            assert_eq!(tu.id, "tool_456");
            assert_eq!(tu.name, "Read");
            assert_eq!(tu.input.get("file_path").unwrap(), "/example.txt");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_with_tool_result() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tool_789", "content": "File contents here"}
        ]}
    }));
    match m {
        Message::User(u) => {
            let blocks = match &u.content {
                UserContent::Blocks(b) => b,
                _ => panic!(),
            };
            assert_eq!(blocks.len(), 1);
            let tr = blocks[0].as_tool_result().unwrap();
            assert_eq!(tr.tool_use_id, "tool_789");
            assert_eq!(tr.content, Some(Value::String("File contents here".into())));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_with_tool_result_error() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tool_error", "content": "File not found", "is_error": true}
        ]}
    }));
    match m {
        Message::User(u) => {
            let tr = match &u.content {
                UserContent::Blocks(b) => b[0].as_tool_result().unwrap(),
                _ => panic!(),
            };
            assert_eq!(tr.tool_use_id, "tool_error");
            assert_eq!(tr.content, Some(Value::String("File not found".into())));
            assert_eq!(tr.is_error, Some(true));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_with_mixed_content() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [
            {"type": "text", "text": "Here's what I found:"},
            {"type": "tool_use", "id": "use_1", "name": "Search", "input": {"query": "test"}},
            {"type": "tool_result", "tool_use_id": "use_1", "content": "Search results"},
            {"type": "text", "text": "What do you think?"}
        ]}
    }));
    match m {
        Message::User(u) => {
            let blocks = match &u.content {
                UserContent::Blocks(b) => b,
                _ => panic!(),
            };
            assert_eq!(blocks.len(), 4);
            assert!(blocks[0].as_text().is_some());
            assert!(blocks[1].as_tool_use().is_some());
            assert!(blocks[2].as_tool_result().is_some());
            assert!(blocks[3].as_text().is_some());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_inside_subagent() {
    let m = parse(json!({
        "type": "user",
        "message": {"content": [{"type": "text", "text": "Hello"}]},
        "parent_tool_use_id": "toolu_01Xrwd5Y13sEHtzScxR77So8"
    }));
    match m {
        Message::User(u) => assert_eq!(
            u.parent_tool_use_id.as_deref(),
            Some("toolu_01Xrwd5Y13sEHtzScxR77So8")
        ),
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_with_tool_use_result() {
    let tool_result_data = json!({
        "filePath": "/path/to/file.py",
        "oldString": "old code",
        "newString": "new code",
        "originalFile": "full file contents",
        "structuredPatch": [{
            "oldStart": 33, "oldLines": 7, "newStart": 33, "newLines": 7,
            "lines": ["   # comment", "-      old line", "+      new line"]
        }],
        "userModified": false,
        "replaceAll": false
    });
    let data = json!({
        "type": "user",
        "message": {"role": "user", "content": [
            {"tool_use_id": "toolu_vrtx_01KXWexk3NJdwkjWzPMGQ2F1", "type": "tool_result", "content": "The file has been updated."}
        ]},
        "parent_tool_use_id": null,
        "session_id": "84afb479-17ae-49af-8f2b-666ac2530c3a",
        "uuid": "2ace3375-1879-48a0-a421-6bce25a9295a",
        "tool_use_result": tool_result_data
    });
    match parse(data) {
        Message::User(u) => {
            assert_eq!(u.tool_use_result, Some(tool_result_data.clone()));
            assert_eq!(
                u.tool_use_result.as_ref().unwrap().get("filePath"),
                Some(&json!("/path/to/file.py"))
            );
            assert_eq!(
                u.uuid.as_deref(),
                Some("2ace3375-1879-48a0-a421-6bce25a9295a")
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_user_message_with_string_content_and_tool_use_result() {
    let tool_result_data = json!({"filePath": "/path/to/file.py", "userModified": true});
    let data = json!({
        "type": "user",
        "message": {"content": "Simple string content"},
        "tool_use_result": tool_result_data
    });
    match parse(data) {
        Message::User(u) => {
            assert_eq!(u.content, UserContent::Text("Simple string content".into()));
            assert_eq!(u.tool_use_result, Some(tool_result_data));
        }
        _ => panic!(),
    }
}

// --------------------------------------------------------- assistant messages

#[test]
fn parse_valid_assistant_message() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "tool_use", "id": "tool_123", "name": "Read", "input": {"file_path": "/test.txt"}}
            ],
            "model": "claude-opus-4-1-20250805"
        }
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.content.len(), 2);
            assert!(a.content[0].as_text().is_some());
            assert!(a.content[1].as_tool_use().is_some());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_thinking() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [
                {"type": "thinking", "thinking": "I'm thinking about the answer...", "signature": "sig-123"},
                {"type": "text", "text": "Here's my response"}
            ],
            "model": "claude-opus-4-1-20250805"
        }
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.content.len(), 2);
            let th = a.content[0].as_thinking().unwrap();
            assert_eq!(th.thinking, "I'm thinking about the answer...");
            assert_eq!(th.signature, "sig-123");
            assert_eq!(a.content[1].as_text().unwrap().text, "Here's my response");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_server_tool_use() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "server_tool_use", "id": "srvtoolu_01ABC", "name": "advisor", "input": {}}],
            "model": "claude-sonnet-4-5"
        }
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.content.len(), 1);
            let su = a.content[0].as_server_tool_use().unwrap();
            assert_eq!(su.id, "srvtoolu_01ABC");
            assert_eq!(su.name, ServerToolName::Advisor);
            assert!(su.input.is_empty());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_skips_unknown_content_block_type() {
    // Unknown block types (e.g. the newer `fallback` block from newer models)
    // must be silently skipped so the rest of the message still parses —
    // matches the Python parser's match-without-default behavior.
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "before"},
                {"type": "fallback", "id": "x", "data": "unknown-shape"},
                {"type": "text", "text": "after"}
            ],
            "model": "claude-opus-4-8"
        }
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.content.len(), 2);
            assert_eq!(a.content[0].as_text().unwrap().text, "before");
            assert_eq!(a.content[1].as_text().unwrap().text, "after");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_all_blocks_unknown() {
    // A message whose only block is unknown still parses (with empty content)
    // rather than raising — the unknown block is dropped.
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "future_block_v2", "payload": 42}],
            "model": "claude-opus-4-9"
        }
    }));
    match m {
        Message::Assistant(a) => assert_eq!(a.content.len(), 0),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_server_tool_result() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{
                "type": "advisor_tool_result",
                "tool_use_id": "srvtoolu_01ABC",
                "content": {"type": "advisor_result", "text": "Consider edge cases around empty input."}
            }],
            "model": "claude-sonnet-4-5"
        }
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.content.len(), 1);
            let sr = a.content[0].as_server_tool_result().unwrap();
            assert_eq!(sr.tool_use_id, "srvtoolu_01ABC");
            assert_eq!(sr.content.get("type").unwrap(), "advisor_result");
            assert_eq!(
                sr.content.get("text").unwrap(),
                "Consider edge cases around empty input."
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_redacted_advisor_result() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{
                "type": "advisor_tool_result",
                "tool_use_id": "srvtoolu_01ABC",
                "content": {"type": "advisor_redacted_result", "encrypted_content": "EuYDCioIDhgC..."}
            }],
            "model": "claude-sonnet-4-5"
        }
    }));
    match m {
        Message::Assistant(a) => {
            let sr = a.content[0].as_server_tool_result().unwrap();
            assert_eq!(
                sr.content.get("type"),
                Some(&json!("advisor_redacted_result"))
            );
            assert_eq!(
                sr.content.get("encrypted_content"),
                Some(&json!("EuYDCioIDhgC..."))
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_usage() {
    let usage = json!({
        "input_tokens": 100, "output_tokens": 50,
        "cache_read_input_tokens": 2000, "cache_creation_input_tokens": 500
    });
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "text", "text": "hi"}],
            "model": "claude-opus-4-5",
            "usage": usage
        }
    }));
    match m {
        Message::Assistant(a) => assert_eq!(a.usage, Some(usage)),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_without_usage() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "hi"}], "model": "claude-opus-4-5"}
    }));
    match m {
        Message::Assistant(a) => assert!(a.usage.is_none()),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_without_error() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "Hello"}], "model": "claude-opus-4-5-20251101"}
    }));
    match m {
        Message::Assistant(a) => assert!(a.error.is_none()),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_authentication_error() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "Invalid API key"}], "model": "<synthetic>"},
        "session_id": "test-session",
        "error": "authentication_failed"
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(a.error, Some(AssistantMessageError::AuthenticationFailed));
            assert_eq!(a.content.len(), 1);
            assert!(a.content[0].as_text().is_some());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_unknown_error() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "API Error: 500"}], "model": "<synthetic>"},
        "session_id": "test-session",
        "error": "unknown"
    }));
    match m {
        Message::Assistant(a) => assert_eq!(a.error, Some(AssistantMessageError::Unknown)),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_rate_limit_error() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "Rate limit exceeded"}], "model": "<synthetic>"},
        "error": "rate_limit"
    }));
    match m {
        Message::Assistant(a) => assert_eq!(a.error, Some(AssistantMessageError::RateLimit)),
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_with_all_fields() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "text", "text": "Hello"}],
            "model": "claude-sonnet-4-5-20250929",
            "id": "msg_01HRq7YZE3apPqSHydvG77Ve",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        },
        "session_id": "fdf2d90a-fd9e-4736-ae35-806edd13643f",
        "uuid": "0dbd2453-1209-4fe9-bd51-4102f64e33df"
    }));
    match m {
        Message::Assistant(a) => {
            assert_eq!(
                a.message_id.as_deref(),
                Some("msg_01HRq7YZE3apPqSHydvG77Ve")
            );
            assert_eq!(a.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(
                a.session_id.as_deref(),
                Some("fdf2d90a-fd9e-4736-ae35-806edd13643f")
            );
            assert_eq!(
                a.uuid.as_deref(),
                Some("0dbd2453-1209-4fe9-bd51-4102f64e33df")
            );
            assert_eq!(
                a.usage,
                Some(json!({"input_tokens": 10, "output_tokens": 5}))
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_optional_fields_absent() {
    let m = parse(json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "hi"}], "model": "claude-opus-4-5"}
    }));
    match m {
        Message::Assistant(a) => {
            assert!(a.message_id.is_none());
            assert!(a.stop_reason.is_none());
            assert!(a.session_id.is_none());
            assert!(a.uuid.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_assistant_message_inside_subagent() {
    let m = parse(json!({
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "tool_use", "id": "tool_123", "name": "Read", "input": {"file_path": "/test.txt"}}
            ],
            "model": "claude-opus-4-1-20250805"
        },
        "parent_tool_use_id": "toolu_01Xrwd5Y13sEHtzScxR77So8"
    }));
    match m {
        Message::Assistant(a) => assert_eq!(
            a.parent_tool_use_id.as_deref(),
            Some("toolu_01Xrwd5Y13sEHtzScxR77So8")
        ),
        _ => panic!(),
    }
}

// ---------------------------------------------------------------- system messages

#[test]
fn parse_valid_system_message() {
    let m = parse(json!({"type": "system", "subtype": "start"}));
    match m {
        Message::System(s) => {
            assert_eq!(s.subtype, "start");
        }
        _ => panic!("expected System, got {:?}", m.variant_name()),
    }
}

#[test]
fn parse_task_started_message() {
    let m = parse(json!({
        "type": "system", "subtype": "task_started",
        "task_id": "task-abc", "tool_use_id": "toolu_01",
        "description": "Reticulating splines", "task_type": "background",
        "uuid": "uuid-1", "session_id": "session-1"
    }));
    match m {
        Message::TaskStarted(t) => {
            assert_eq!(t.task_id, "task-abc");
            assert_eq!(t.description, "Reticulating splines");
            assert_eq!(t.uuid, "uuid-1");
            assert_eq!(t.session_id, "session-1");
            assert_eq!(t.tool_use_id.as_deref(), Some("toolu_01"));
            assert_eq!(t.task_type.as_deref(), Some("background"));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_started_message_optional_fields_absent() {
    let m = parse(json!({
        "type": "system", "subtype": "task_started",
        "task_id": "task-abc", "description": "Working",
        "uuid": "uuid-1", "session_id": "session-1"
    }));
    match m {
        Message::TaskStarted(t) => {
            assert!(t.tool_use_id.is_none());
            assert!(t.task_type.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_non_dict_patch() {
    // Non-dict / null patch never raises; patch falls back to {}.
    // Mirrors Python @pytest.mark.parametrize: "completed", ["completed"], 42, None.
    let cases: [Value; 4] = [
        json!("completed"),
        json!(["completed"]),
        json!(42),
        Value::Null,
    ];
    for val in cases {
        let m = parse(json!({
            "type": "system", "subtype": "task_updated", "task_id": "task-abc",
            "patch": val
        }));
        match m {
            Message::TaskUpdated(t) => {
                assert!(t.patch.is_empty(), "patch should be empty for {val}");
                assert!(t.status.is_none());
            }
            _ => panic!(),
        }
    }
}

#[test]
fn parse_task_notification_message() {
    let m = parse(json!({
        "type": "system", "subtype": "task_notification",
        "task_id": "task-abc", "tool_use_id": "toolu_01",
        "status": "completed", "output_file": "/tmp/out.md", "summary": "All done",
        "usage": {"total_tokens": 2000, "tool_uses": 7, "duration_ms": 12345},
        "uuid": "uuid-3", "session_id": "session-1"
    }));
    match m {
        Message::TaskNotification(t) => {
            assert_eq!(t.task_id, "task-abc");
            assert_eq!(t.status, TaskNotificationStatus::Completed);
            assert_eq!(t.output_file, "/tmp/out.md");
            assert_eq!(t.summary, "All done");
            assert_eq!(t.usage.as_ref().unwrap().total_tokens, 2000);
            assert_eq!(t.tool_use_id.as_deref(), Some("toolu_01"));
            assert_eq!(t.uuid, "uuid-3");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_notification_message_optional_fields_absent() {
    let m = parse(json!({
        "type": "system", "subtype": "task_notification",
        "task_id": "task-abc", "status": "failed",
        "output_file": "/tmp/out.md", "summary": "Boom",
        "uuid": "uuid-3", "session_id": "session-1"
    }));
    match m {
        Message::TaskNotification(t) => {
            assert_eq!(t.status, TaskNotificationStatus::Failed);
            assert!(t.usage.is_none());
            assert!(t.tool_use_id.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_terminal() {
    let m = parse(json!({
        "type": "system", "subtype": "task_updated",
        "task_id": "task-abc",
        "patch": {"status": "completed", "end_time": 1780405729183_i64},
        "uuid": "uuid-4", "session_id": "session-1"
    }));
    match m {
        Message::TaskUpdated(t) => {
            assert_eq!(t.task_id, "task-abc");
            assert_eq!(t.patch.get("status").unwrap(), "completed");
            assert_eq!(t.patch.get("end_time").unwrap(), 1780405729183_i64);
            assert_eq!(t.status, Some(TaskUpdatedStatus::Completed));
            assert_eq!(t.uuid.as_deref(), Some("uuid-4"));
            assert_eq!(t.session_id.as_deref(), Some("session-1"));
            assert!(claude_agent_sdk::terminal_task_statuses().contains("completed"));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_minimal() {
    let m = parse(json!({
        "type": "system", "subtype": "task_updated",
        "task_id": "b1m21w89v",
        "patch": {"status": "completed", "end_time": 1780405729183_i64}
    }));
    match m {
        Message::TaskUpdated(t) => {
            assert_eq!(t.task_id, "b1m21w89v");
            assert_eq!(t.status, Some(TaskUpdatedStatus::Completed));
            assert!(t.uuid.is_none());
            assert!(t.session_id.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_non_terminal_statuses() {
    for status in ["pending", "running", "paused"] {
        let m = parse(json!({
            "type": "system", "subtype": "task_updated",
            "task_id": "task-abc", "patch": {"status": status}
        }));
        match m {
            Message::TaskUpdated(t) => {
                let expected = TaskUpdatedStatus::from_str_lossy(status).unwrap();
                assert_eq!(t.status, Some(expected));
                assert!(!claude_agent_sdk::terminal_task_statuses().contains(status));
            }
            _ => panic!(),
        }
    }
}

#[test]
fn parse_task_updated_message_no_patch() {
    let m = parse(json!({"type": "system", "subtype": "task_updated", "task_id": "task-abc"}));
    match m {
        Message::TaskUpdated(t) => {
            assert!(t.patch.is_empty());
            assert!(t.status.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_patch_without_status() {
    let m = parse(json!({
        "type": "system", "subtype": "task_updated", "task_id": "task-abc",
        "patch": {"end_time": 1780405729183_i64}
    }));
    match m {
        Message::TaskUpdated(t) => {
            assert_eq!(t.patch.get("end_time").unwrap(), 1780405729183_i64);
            assert!(t.status.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_task_updated_message_terminal_statuses() {
    for status in ["completed", "failed", "killed"] {
        let m = parse(json!({
            "type": "system", "subtype": "task_updated", "task_id": "task-abc",
            "patch": {"status": status}
        }));
        match m {
            Message::TaskUpdated(t) => {
                assert_eq!(t.status, TaskUpdatedStatus::from_str_lossy(status));
                assert!(claude_agent_sdk::terminal_task_statuses().contains(status));
            }
            _ => panic!(),
        }
    }
}

#[test]
fn parse_task_updated_killed_is_terminal() {
    let m = parse(json!({
        "type": "system", "subtype": "task_updated", "task_id": "bs2r8eew4",
        "patch": {"status": "killed", "end_time": 1780405729183_i64}
    }));
    match m {
        Message::TaskUpdated(t) => {
            assert_eq!(t.status, Some(TaskUpdatedStatus::Killed));
            assert!(claude_agent_sdk::terminal_task_statuses().contains("killed"));
        }
        _ => panic!(),
    }
}

#[test]
fn task_updated_backward_compat_is_system() {
    let m = parse(json!({
        "type": "system", "subtype": "task_updated", "task_id": "t1",
        "patch": {"status": "failed"}, "uuid": "u1", "session_id": "s1"
    }));
    assert!(m.is_system());
    let view = m.as_system().unwrap();
    assert_eq!(view.subtype, "task_updated");
    assert!(matches!(m, Message::TaskUpdated(_)));
}

#[test]
fn task_message_backward_compat_is_system() {
    let started = parse(json!({
        "type": "system", "subtype": "task_started", "task_id": "t1",
        "description": "desc", "uuid": "u1", "session_id": "s1"
    }));
    let progress = parse(json!({
        "type": "system", "subtype": "task_progress", "task_id": "t1",
        "description": "desc", "usage": {"total_tokens": 1, "tool_uses": 0, "duration_ms": 10},
        "uuid": "u2", "session_id": "s1"
    }));
    let notif = parse(json!({
        "type": "system", "subtype": "task_notification", "task_id": "t1",
        "status": "stopped", "output_file": "/o", "summary": "s",
        "uuid": "u3", "session_id": "s1"
    }));
    assert!(started.is_system());
    assert!(progress.is_system());
    assert!(notif.is_system());
}

#[test]
fn task_message_backward_compat_base_fields() {
    let data = json!({
        "type": "system", "subtype": "task_started", "task_id": "t1",
        "description": "desc", "uuid": "u1", "session_id": "s1"
    });
    let m = parse(data.clone());
    match &m {
        Message::TaskStarted(t) => {
            assert_eq!(t.subtype, "task_started");
            assert_eq!(t.data, data);
        }
        _ => panic!(),
    }
    // base view still works
    let view = m.as_system().unwrap();
    assert_eq!(view.subtype, "task_started");
    assert_eq!(view.data, &data);
}

#[test]
fn unknown_system_subtype_yields_generic() {
    let data = json!({"type": "system", "subtype": "some_future_subtype", "foo": "bar"});
    let m = parse(data.clone());
    match &m {
        Message::System(s) => {
            assert_eq!(s.subtype, "some_future_subtype");
            assert_eq!(s.data, data);
        }
        _ => panic!("expected exactly SystemMessage, got {:?}", m.variant_name()),
    }
    // Ensure it's NOT one of the typed subclasses.
    assert!(!matches!(m, Message::TaskStarted(_)));
    assert!(!matches!(m, Message::TaskProgress(_)));
    assert!(!matches!(m, Message::TaskNotification(_)));
    assert!(!matches!(m, Message::TaskUpdated(_)));
}

// -------------------------------------------------------------- result messages

#[test]
fn parse_valid_result_message() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 1000, "duration_api_ms": 500,
        "is_error": false, "num_turns": 2, "session_id": "session_123"
    }));
    match m {
        Message::Result(r) => {
            assert_eq!(r.subtype, "success");
            assert!(r.stop_reason.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_stop_reason() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 1000, "duration_api_ms": 500,
        "is_error": false, "num_turns": 2, "session_id": "session_123",
        "stop_reason": "end_turn", "result": "Done"
    }));
    match m {
        Message::Result(r) => {
            assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(r.result.as_deref(), Some("Done"));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_null_stop_reason() {
    let m = parse(json!({
        "type": "result", "subtype": "error_max_turns",
        "duration_ms": 1000, "duration_api_ms": 500,
        "is_error": true, "num_turns": 10, "session_id": "session_123",
        "stop_reason": null
    }));
    match m {
        Message::Result(r) => assert!(r.stop_reason.is_none()),
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_model_usage() {
    let model_usage = json!({
        "claude-sonnet-4-5-20250929": {
            "inputTokens": 3, "outputTokens": 24,
            "cacheReadInputTokens": 20012, "costUSD": 0.0106,
            "contextWindow": 200000, "maxOutputTokens": 64000
        }
    });
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 3000, "duration_api_ms": 2000,
        "is_error": false, "num_turns": 1,
        "session_id": "fdf2d90a-fd9e-4736-ae35-806edd13643f",
        "stop_reason": "end_turn", "total_cost_usd": 0.0106,
        "usage": {"input_tokens": 3, "output_tokens": 24},
        "result": "Hello", "modelUsage": model_usage,
        "permission_denials": [],
        "uuid": "d379c496-f33a-4ea4-b920-3c5483baa6f7"
    }));
    match m {
        Message::Result(r) => {
            let mu = r.model_usage.as_ref().unwrap();
            let entry = mu.get("claude-sonnet-4-5-20250929").unwrap();
            assert_eq!(entry.get("costUSD").unwrap(), 0.0106);
            assert_eq!(r.permission_denials.as_ref().unwrap().len(), 0);
            assert_eq!(
                r.uuid.as_deref(),
                Some("d379c496-f33a-4ea4-b920-3c5483baa6f7")
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_optional_fields_absent() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 1000, "duration_api_ms": 500,
        "is_error": false, "num_turns": 1, "session_id": "session_123"
    }));
    match m {
        Message::Result(r) => {
            assert!(r.model_usage.is_none());
            assert!(r.permission_denials.is_none());
            assert!(r.deferred_tool_use.is_none());
            assert!(r.errors.is_none());
            assert!(r.api_error_status.is_none());
            assert!(r.uuid.is_none());
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_deferred_tool_use() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 1200, "duration_api_ms": 900,
        "is_error": false, "num_turns": 1, "session_id": "session_123",
        "deferred_tool_use": {
            "id": "toolu_01abc", "name": "Bash", "input": {"command": "rm -rf /tmp/scratch"}
        }
    }));
    match m {
        Message::Result(r) => {
            let d = r.deferred_tool_use.as_ref().unwrap();
            assert_eq!(d.id, "toolu_01abc");
            assert_eq!(d.name, "Bash");
            assert_eq!(d.input.get("command").unwrap(), "rm -rf /tmp/scratch");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_errors() {
    let m = parse(json!({
        "type": "result", "subtype": "error_during_execution",
        "duration_ms": 5000, "duration_api_ms": 3000,
        "is_error": true, "num_turns": 3, "session_id": "session_456",
        "errors": ["Tool execution failed: permission denied", "Unable to write to /etc/hosts"],
        "uuid": "err-uuid-789"
    }));
    match m {
        Message::Result(r) => {
            assert_eq!(
                r.errors.as_ref().unwrap(),
                &[
                    "Tool execution failed: permission denied",
                    "Unable to write to /etc/hosts"
                ]
            );
            assert!(r.is_error);
            assert_eq!(r.subtype, "error_during_execution");
            assert_eq!(r.uuid.as_deref(), Some("err-uuid-789"));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_with_api_error_status() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 2000, "duration_api_ms": 1500,
        "is_error": true, "num_turns": 1, "session_id": "session_overload",
        "api_error_status": 529
    }));
    match m {
        Message::Result(r) => {
            assert_eq!(r.api_error_status, Some(529));
            assert!(r.is_error);
            assert_eq!(r.subtype, "success");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_result_message_success_no_errors() {
    let m = parse(json!({
        "type": "result", "subtype": "success",
        "duration_ms": 1000, "duration_api_ms": 500,
        "is_error": false, "num_turns": 1, "session_id": "session_789",
        "result": "Task completed successfully"
    }));
    match m {
        Message::Result(r) => {
            assert!(r.errors.is_none());
            assert_eq!(r.result.as_deref(), Some("Task completed successfully"));
        }
        _ => panic!(),
    }
}

// --------------------------------------------------------------- rate limit event

#[test]
fn parse_rate_limit_event() {
    let m = parse(json!({
        "type": "rate_limit_event",
        "rate_limit_info": {
            "status": "allowed_warning", "resetsAt": 1700000000,
            "rateLimitType": "five_hour", "utilization": 0.91
        },
        "uuid": "abc-123", "session_id": "session_xyz"
    }));
    match m {
        Message::RateLimitEvent(e) => {
            assert_eq!(e.uuid, "abc-123");
            assert_eq!(e.session_id, "session_xyz");
            assert_eq!(
                e.rate_limit_info.status,
                claude_agent_sdk::RateLimitStatus::AllowedWarning
            );
            assert_eq!(e.rate_limit_info.resets_at, Some(1700000000));
            assert_eq!(
                e.rate_limit_info.rate_limit_type,
                Some(claude_agent_sdk::RateLimitType::FiveHour)
            );
            assert_eq!(e.rate_limit_info.utilization, Some(0.91));
        }
        _ => panic!(),
    }
}

#[test]
fn parse_rate_limit_event_preserves_unmodeled_fields_in_raw() {
    // Mirrors test_rate_limit_event_repro: isUsingOverage is not modeled but
    // must survive in `raw` so callers can inspect it.
    let m = parse(json!({
        "type": "rate_limit_event",
        "rate_limit_info": {
            "status": "allowed_warning", "resetsAt": 1700000000,
            "rateLimitType": "five_hour", "utilization": 0.85,
            "isUsingOverage": false
        },
        "uuid": "550e8400-e29b-41d4-a716-446655440000",
        "session_id": "test-session-id"
    }));
    match m {
        Message::RateLimitEvent(e) => {
            assert_eq!(e.uuid, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(e.session_id, "test-session-id");
            assert_eq!(e.rate_limit_info.utilization, Some(0.85));
            assert_eq!(e.rate_limit_info.raw.get("isUsingOverage").unwrap(), false);
        }
        _ => panic!(),
    }
}

#[test]
fn parse_rate_limit_event_rejected_with_overage() {
    // Hard rate limit (status=rejected) with overage details.
    let m = parse(json!({
        "type": "rate_limit_event",
        "rate_limit_info": {
            "status": "rejected", "resetsAt": 1700003600,
            "rateLimitType": "seven_day", "isUsingOverage": false,
            "overageStatus": "rejected", "overageDisabledReason": "out_of_credits"
        },
        "uuid": "660e8400-e29b-41d4-a716-446655440001",
        "session_id": "test-session-id"
    }));
    match m {
        Message::RateLimitEvent(e) => {
            assert_eq!(
                e.rate_limit_info.status,
                claude_agent_sdk::RateLimitStatus::Rejected
            );
            assert_eq!(
                e.rate_limit_info.rate_limit_type,
                Some(claude_agent_sdk::RateLimitType::SevenDay)
            );
            assert_eq!(
                e.rate_limit_info.overage_status,
                Some(claude_agent_sdk::RateLimitStatus::Rejected)
            );
            assert_eq!(
                e.rate_limit_info.overage_disabled_reason.as_deref(),
                Some("out_of_credits")
            );
        }
        _ => panic!(),
    }
}

#[test]
fn parse_rate_limit_event_minimal_fields() {
    // Only status is required; optional fields default to None.
    let m = parse(json!({
        "type": "rate_limit_event",
        "rate_limit_info": {"status": "allowed"},
        "uuid": "770e8400-e29b-41d4-a716-446655440002",
        "session_id": "test-session-id"
    }));
    match m {
        Message::RateLimitEvent(e) => {
            assert_eq!(
                e.rate_limit_info.status,
                claude_agent_sdk::RateLimitStatus::Allowed
            );
            assert!(e.rate_limit_info.resets_at.is_none());
            assert!(e.rate_limit_info.rate_limit_type.is_none());
        }
        _ => panic!(),
    }
}

// ----------------------------------------------------------------- hook events

#[test]
fn parse_hook_event_message() {
    let data = json!({
        "type": "system", "subtype": "hook_started",
        "hook_event": "PreToolUse", "hook_name": "PreToolUse",
        "session_id": "sess-123", "uuid": "uuid-456",
        "tool_name": "Bash", "tool_input": {"command": "ls"}
    });
    let m = parse(data.clone());
    match &m {
        Message::HookEvent(h) => {
            assert_eq!(h.subtype, "hook_started");
            assert_eq!(h.hook_event_name, "PreToolUse");
            assert_eq!(h.session_id.as_deref(), Some("sess-123"));
            assert_eq!(h.uuid.as_deref(), Some("uuid-456"));
            assert_eq!(h.data, data);
        }
        _ => panic!(),
    }
}

#[test]
fn parse_hook_event_message_response() {
    let data = json!({
        "type": "system", "subtype": "hook_response",
        "hook_event": "PostToolUse", "hook_name": "PostToolUse",
        "session_id": "sess-123", "uuid": "uuid-789",
        "output": "", "exit_code": 0, "outcome": "success"
    });
    let m = parse(data.clone());
    match &m {
        Message::HookEvent(h) => {
            assert_eq!(h.subtype, "hook_response");
            assert_eq!(h.hook_event_name, "PostToolUse");
            assert_eq!(h.session_id.as_deref(), Some("sess-123"));
            assert_eq!(h.uuid.as_deref(), Some("uuid-789"));
            assert_eq!(h.data.get("output").unwrap(), "");
            assert_eq!(h.data.get("exit_code").unwrap(), 0);
            assert_eq!(h.data.get("outcome").unwrap(), "success");
        }
        _ => panic!(),
    }
}

#[test]
fn parse_hook_event_message_is_system() {
    let m = parse(json!({"type": "system", "subtype": "hook_started", "hook_event": "PreToolUse"}));
    assert!(matches!(m, Message::HookEvent(_)));
    assert!(m.is_system());
}

#[test]
fn parse_hook_event_message_minimal() {
    let m = parse(json!({"type": "system", "subtype": "hook_started", "hook_name": "Stop"}));
    match m {
        Message::HookEvent(h) => {
            assert_eq!(h.subtype, "hook_started");
            assert_eq!(h.hook_event_name, "Stop");
            assert!(h.session_id.is_none());
            assert!(h.uuid.is_none());
        }
        _ => panic!(),
    }
}

// ---------------------------------------------------------------- error handling

#[test]
fn parse_invalid_data_type() {
    let e = parse_err(json!("not a dict"));
    assert!(
        e.message.contains("Invalid message data type"),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("expected dict, got string"),
        "{}",
        e.message
    );
}

#[test]
fn parse_missing_type_field() {
    let e = parse_err(json!({"message": {"content": []}}));
    assert!(
        e.message.contains("Message missing 'type' field"),
        "{}",
        e.message
    );
}

#[test]
fn parse_unknown_message_type_returns_none() {
    parse_none(json!({"type": "unknown_type"}));
}

#[test]
fn parse_user_message_missing_fields() {
    let e = parse_err(json!({"type": "user"}));
    assert!(
        e.message.contains("Missing required field in user message"),
        "{}",
        e.message
    );
}

#[test]
fn parse_assistant_message_missing_fields() {
    let e = parse_err(json!({"type": "assistant"}));
    assert!(
        e.message
            .contains("Missing required field in assistant message"),
        "{}",
        e.message
    );
}

#[test]
fn parse_system_message_missing_fields() {
    let e = parse_err(json!({"type": "system"}));
    assert!(
        e.message
            .contains("Missing required field in system message"),
        "{}",
        e.message
    );
}

#[test]
fn parse_result_message_missing_fields() {
    let e = parse_err(json!({"type": "result", "subtype": "success"}));
    assert!(
        e.message
            .contains("Missing required field in result message"),
        "{}",
        e.message
    );
}

#[test]
fn message_parse_error_contains_data() {
    let data = json!({"type": "assistant"});
    let e = parse_err(data.clone());
    assert_eq!(e.data, Some(data));
}

// ------------------------------------------------------------------ type markers

trait MessageVariant {
    fn variant_name(&self) -> &'static str;
}

impl MessageVariant for Message {
    fn variant_name(&self) -> &'static str {
        match self {
            Message::User(_) => "User",
            Message::Assistant(_) => "Assistant",
            Message::System(_) => "System",
            Message::TaskStarted(_) => "TaskStarted",
            Message::TaskProgress(_) => "TaskProgress",
            Message::TaskNotification(_) => "TaskNotification",
            Message::TaskUpdated(_) => "TaskUpdated",
            Message::MirrorError(_) => "MirrorError",
            Message::HookEvent(_) => "HookEvent",
            Message::Result(_) => "Result",
            Message::StreamEvent(_) => "StreamEvent",
            Message::RateLimitEvent(_) => "RateLimitEvent",
        }
    }
}
