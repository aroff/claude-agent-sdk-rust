//! Integration tests for Claude SDK type definitions, mirroring
//! `tests/test_types.py` from the Python SDK.

use claude_agent_sdk::{
    AssistantMessage, EffortLevel, PermissionBehavior, PermissionMode, PermissionRuleValue,
    PermissionUpdate, PermissionUpdateDestination, PermissionUpdateType, ResultMessage, TextBlock,
    ThinkingBlock, ToolResultBlock, ToolUseBlock, UserContent, UserMessage,
};
use serde_json::{json, Map, Value};

// ------------------------------------------------------------- effort level

#[test]
fn effort_level_is_exported() {
    assert_eq!(
        EffortLevel::variants(),
        ["low", "medium", "high", "xhigh", "max"]
    );
}

// ------------------------------------------------------- PermissionUpdate

fn m(json_str: &str) -> Map<String, Value> {
    let v: Value = serde_json::from_str(json_str).unwrap();
    v.as_object().unwrap().clone()
}

#[test]
fn from_dict_to_dict_roundtrip_add_rules() {
    let wire = m(r#"{
            "type": "addRules",
            "destination": "localSettings",
            "behavior": "allow",
            "rules": [
                {"toolName": "Bash", "ruleContent": "npm *"},
                {"toolName": "Read", "ruleContent": null}
            ]
        }"#);
    let update = PermissionUpdate::from_dict(&wire).unwrap();
    assert_eq!(update.r#type, Some(PermissionUpdateType::AddRules));
    assert_eq!(
        update.destination,
        Some(PermissionUpdateDestination::LocalSettings)
    );
    assert_eq!(update.behavior, Some(PermissionBehavior::Allow));
    assert_eq!(
        update.rules,
        Some(vec![
            PermissionRuleValue {
                tool_name: "Bash".into(),
                rule_content: Some("npm *".into()),
            },
            PermissionRuleValue {
                tool_name: "Read".into(),
                rule_content: None,
            },
        ])
    );
    assert_eq!(Value::Object(update.to_dict()), Value::Object(wire));
}

#[test]
fn from_dict_set_mode() {
    let wire = m(r#"{"type": "setMode", "mode": "acceptEdits", "destination": "session"}"#);
    let update = PermissionUpdate::from_dict(&wire).unwrap();
    assert_eq!(update.mode, Some(PermissionMode::AcceptEdits));
    assert!(update.rules.is_none());
    assert_eq!(Value::Object(update.to_dict()), Value::Object(wire));
}

#[test]
fn from_dict_directories() {
    let wire = m(r#"{
            "type": "addDirectories",
            "directories": ["/tmp/a", "/tmp/b"],
            "destination": "userSettings"
        }"#);
    let update = PermissionUpdate::from_dict(&wire).unwrap();
    assert_eq!(
        update.directories,
        Some(vec!["/tmp/a".into(), "/tmp/b".into()])
    );
    assert_eq!(Value::Object(update.to_dict()), Value::Object(wire));
}

// ----------------------------------------------------------- Message types

#[test]
fn user_message_creation() {
    let msg = UserMessage {
        content: UserContent::Text("Hello, Claude!".into()),
        uuid: None,
        parent_tool_use_id: None,
        tool_use_result: None,
    };
    assert_eq!(msg.content, UserContent::Text("Hello, Claude!".into()));
}

#[test]
fn assistant_message_with_text() {
    let msg = AssistantMessage {
        content: vec![claude_agent_sdk::ContentBlock::Text(TextBlock {
            text: "Hello, human!".into(),
        })],
        model: "claude-opus-4-1-20250805".into(),
        parent_tool_use_id: None,
        error: None,
        usage: None,
        message_id: None,
        stop_reason: None,
        session_id: None,
        uuid: None,
    };
    assert_eq!(msg.content.len(), 1);
    assert_eq!(msg.content[0].as_text().unwrap().text, "Hello, human!");
}

#[test]
fn assistant_message_with_thinking() {
    let block = ThinkingBlock {
        thinking: "I'm thinking...".into(),
        signature: "sig-123".into(),
    };
    let msg = AssistantMessage {
        content: vec![claude_agent_sdk::ContentBlock::Thinking(block)],
        model: "claude-opus-4-1-20250805".into(),
        parent_tool_use_id: None,
        error: None,
        usage: None,
        message_id: None,
        stop_reason: None,
        session_id: None,
        uuid: None,
    };
    assert_eq!(msg.content.len(), 1);
    assert_eq!(
        msg.content[0].as_thinking().unwrap().thinking,
        "I'm thinking..."
    );
    assert_eq!(msg.content[0].as_thinking().unwrap().signature, "sig-123");
}

#[test]
fn tool_use_block() {
    let block = ToolUseBlock {
        id: "tool-123".into(),
        name: "Read".into(),
        input: json!({"file_path": "/test.txt"})
            .as_object()
            .unwrap()
            .clone(),
    };
    assert_eq!(block.id, "tool-123");
    assert_eq!(block.name, "Read");
    assert_eq!(block.input.get("file_path").unwrap(), "/test.txt");
}

#[test]
fn tool_result_block() {
    let block = ToolResultBlock {
        tool_use_id: "tool-123".into(),
        content: Some(Value::String("File contents here".into())),
        is_error: Some(false),
    };
    assert_eq!(block.tool_use_id, "tool-123");
    assert_eq!(
        block.content,
        Some(Value::String("File contents here".into()))
    );
    assert_eq!(block.is_error, Some(false));
}

#[test]
fn result_message() {
    let msg = ResultMessage {
        subtype: "success".into(),
        duration_ms: 1500,
        duration_api_ms: 1200,
        is_error: false,
        num_turns: 1,
        session_id: "session-123".into(),
        stop_reason: None,
        total_cost_usd: Some(0.01),
        usage: None,
        result: None,
        structured_output: None,
        model_usage: None,
        permission_denials: None,
        deferred_tool_use: None,
        errors: None,
        api_error_status: None,
        uuid: None,
    };
    assert_eq!(msg.subtype, "success");
    assert_eq!(msg.total_cost_usd, Some(0.01));
    assert_eq!(msg.session_id, "session-123");
}

// ----------------------------------------------- serialization round-trips

#[test]
fn text_block_round_trip() {
    let block = TextBlock { text: "hi".into() };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v, json!({"text": "hi"}));
    let back: TextBlock = serde_json::from_value(v).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_serde_tags() {
    let blocks = vec![
        claude_agent_sdk::ContentBlock::Text(TextBlock { text: "a".into() }),
        claude_agent_sdk::ContentBlock::ToolUse(ToolUseBlock {
            id: "1".into(),
            name: "Read".into(),
            input: Map::new(),
        }),
    ];
    let v = serde_json::to_value(&blocks).unwrap();
    assert_eq!(v[0]["type"], "text");
    assert_eq!(v[1]["type"], "tool_use");
    assert_eq!(v[1]["id"], "1");
}

#[test]
fn permission_mode_round_trip() {
    for mode in [
        PermissionMode::Default,
        PermissionMode::AcceptEdits,
        PermissionMode::Plan,
        PermissionMode::BypassPermissions,
        PermissionMode::DontAsk,
        PermissionMode::Auto,
    ] {
        let v = serde_json::to_value(&mode).unwrap();
        let s = v.as_str().unwrap().to_string();
        let back: PermissionMode = serde_json::from_value(v).unwrap();
        assert_eq!(back, mode, "round-trip failed for {s}");
    }
}
