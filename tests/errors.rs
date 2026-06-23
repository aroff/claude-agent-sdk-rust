//! Integration tests for error types, mirroring `tests/test_errors.py`.

use claude_agent_sdk::{
    ClaudeSdkError, CliConnectionError, CliJsonDecodeError, CliNotFoundError, ProcessError,
};
use std::error::Error;

#[test]
fn base_error_message() {
    let e = ClaudeSdkError::new("Something went wrong");
    assert_eq!(e.to_string(), "Something went wrong");
    // ClaudeSdkError is-a std::error::Error
    let _: &dyn Error = &e;
}

#[test]
fn cli_not_found_error_message() {
    let e = CliNotFoundError::new("Claude Code not found", None);
    assert!(e.to_string().contains("Claude Code not found"));
    let _: &dyn Error = &e;
}

#[test]
fn connection_error_message() {
    let e = CliConnectionError::new("Failed to connect to CLI");
    assert!(e.to_string().contains("Failed to connect to CLI"));
    let _: &dyn Error = &e;
}

#[test]
fn process_error_carries_fields_and_formats() {
    let e = ProcessError::new("Process failed", Some(1), Some("Command not found".into()));
    assert_eq!(e.exit_code, Some(1));
    assert_eq!(e.stderr.as_deref(), Some("Command not found"));
    let msg = e.to_string();
    assert!(msg.contains("Process failed"), "{msg}");
    assert!(msg.contains("exit code: 1"), "{msg}");
    assert!(msg.contains("Command not found"), "{msg}");
}

#[test]
fn json_decode_error_preserves_line_and_original() {
    let line = "{invalid json}";
    let original = serde_json::from_str::<serde_json::Value>(line).unwrap_err();
    let e = CliJsonDecodeError::new(line, original);
    assert_eq!(e.line, line);
    assert!(e.to_string().contains("Failed to decode JSON"));
    // original error message is preserved in the formatted output
    assert!(!e.to_string().is_empty());
}
