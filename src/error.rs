//! Error types for the Claude Agent SDK.
//!
//! Mirrors the Python `claude_agent_sdk._errors` module.

use serde_json::Value;
use std::fmt;

/// Base error for all Claude SDK errors.
#[derive(Debug)]
pub struct ClaudeSdkError {
    pub message: String,
}

impl ClaudeSdkError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ClaudeSdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClaudeSdkError {}

// Blanket conversions from each specific error to the base ClaudeSdkError,
// so `?` works in any function returning `Result<_, ClaudeSdkError>`.
impl From<CliConnectionError> for ClaudeSdkError {
    fn from(e: CliConnectionError) -> Self {
        Self::new(e.message)
    }
}
impl From<CliNotFoundError> for ClaudeSdkError {
    fn from(e: CliNotFoundError) -> Self {
        Self::new(e.message)
    }
}
impl From<ProcessError> for ClaudeSdkError {
    fn from(e: ProcessError) -> Self {
        Self::new(e.message)
    }
}
impl From<MessageParseError> for ClaudeSdkError {
    fn from(e: MessageParseError) -> Self {
        Self::new(e.message)
    }
}
impl From<CliJsonDecodeError> for ClaudeSdkError {
    fn from(e: CliJsonDecodeError) -> Self {
        Self::new(e.original_message)
    }
}

/// Raised when unable to connect to Claude Code.
#[derive(Debug)]
pub struct CliConnectionError {
    pub message: String,
}

impl CliConnectionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliConnectionError {}

/// Raised when Claude Code is not found or not installed.
#[derive(Debug)]
pub struct CliNotFoundError {
    pub message: String,
    pub cli_path: Option<String>,
}

impl CliNotFoundError {
    pub fn new(message: impl Into<String>, cli_path: Option<String>) -> Self {
        let mut message = message.into();
        if let Some(path) = &cli_path {
            message = format!("{message}: {path}");
        }
        Self { message, cli_path }
    }

    pub fn default_msg() -> Self {
        Self::new("Claude Code not found", None)
    }
}

impl fmt::Display for CliNotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliNotFoundError {}

/// Raised when the CLI process fails.
#[derive(Debug)]
pub struct ProcessError {
    pub message: String,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

impl ProcessError {
    pub fn new(message: impl Into<String>, exit_code: Option<i32>, stderr: Option<String>) -> Self {
        let mut message = message.into();
        if let Some(code) = exit_code {
            message = format!("{message} (exit code: {code})");
        }
        if let Some(err) = &stderr {
            message = format!("{message}\nError output: {err}");
        }
        Self {
            message,
            exit_code,
            stderr,
        }
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProcessError {}

/// Raised when unable to decode JSON from CLI output.
#[derive(Debug)]
pub struct CliJsonDecodeError {
    pub line: String,
    pub original_message: String,
}

impl CliJsonDecodeError {
    pub fn new(line: impl Into<String>, original_error: impl std::error::Error) -> Self {
        let line = line.into();
        let prefix: String = line.chars().take(100).collect();
        Self {
            line,
            original_message: format!("Failed to decode JSON: {prefix}... [{}]", original_error),
        }
    }
}

impl fmt::Display for CliJsonDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.original_message)
    }
}

impl std::error::Error for CliJsonDecodeError {}

/// Raised when unable to parse a message from CLI output.
#[derive(Debug)]
pub struct MessageParseError {
    pub message: String,
    pub data: Option<Value>,
}

impl MessageParseError {
    pub fn new(message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            message: message.into(),
            data,
        }
    }
}

impl fmt::Display for MessageParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MessageParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_not_found_default_includes_message() {
        let e = CliNotFoundError::default_msg();
        assert_eq!(e.message, "Claude Code not found");
        assert!(e.cli_path.is_none());
    }

    #[test]
    fn cli_not_found_with_path_appends_path() {
        let e = CliNotFoundError::new("Claude Code not found", Some("/usr/bin/claude".into()));
        assert_eq!(e.message, "Claude Code not found: /usr/bin/claude");
    }

    #[test]
    fn process_error_formats_exit_code_and_stderr() {
        let e = ProcessError::new("boom", Some(2), Some("oops".into()));
        assert_eq!(e.message, "boom (exit code: 2)\nError output: oops");
    }

    #[test]
    fn message_parse_error_stores_data() {
        let data = serde_json::json!({"type": "assistant"});
        let e = MessageParseError::new(
            "Missing required field in assistant message",
            Some(data.clone()),
        );
        assert_eq!(e.data, Some(data));
    }
}
