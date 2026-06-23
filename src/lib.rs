//! Rust port of the Claude Agent SDK with 1:1 Claude message compatibility.
//!
//! This crate mirrors the message types and parsing behavior of the Python
//! `claude_agent_sdk` package so that wire payloads produced by the Claude
//! Code CLI parse identically in both runtimes.
//!
//! ## Core entry points
//!
//! - [`parse_message`] — parse a raw JSON value (one CLI output line) into a
//!   typed [`Message`]. Returns `Ok(None)` for forward-compatible unknown
//!   types and [`MessageParseError`] for malformed payloads.
//! - [`Message`] — the discriminated union of all CLI message variants.
//!
//! ## Example
//!
//! ```
//! use claude_agent_sdk::{parse_message, Message};
//! use serde_json::json;
//!
//! let data = json!({
//!     "type": "assistant",
//!     "message": {
//!         "content": [{"type": "text", "text": "Hello"}],
//!         "model": "claude-opus-4-5"
//!     }
//! });
//! let msg = parse_message(&data).unwrap().unwrap();
//! match msg {
//!     Message::Assistant(a) => assert_eq!(a.content[0].as_text().unwrap().text, "Hello"),
//!     _ => panic!("expected assistant"),
//! }
//! ```

pub mod client;
pub mod control;
pub mod error;
pub mod message_parser;
pub mod options;
pub mod query;
pub mod sdk_mcp;
pub mod session;
pub mod transport;
pub mod types;

pub use client::ClaudeSDKClient;
pub use control::{
    CanUseToolCallback, HookCallback, HookMatcherConfig, McpServerHandler, PermissionResult, Query,
    QueryConfig, ToolPermissionContext,
};
pub use error::{
    ClaudeSdkError, CliConnectionError, CliJsonDecodeError, CliNotFoundError, MessageParseError,
    ProcessError,
};
pub use message_parser::parse_message;
pub use options::{
    compare_versions, permission_mode_str, ClaudeAgentOptions, Effort, McpServers, SdkPluginConfig,
    SkillsFilter, SystemPrompt, SystemPromptFile, SystemPromptPreset, TaskBudget, ThinkingConfig,
    Tools, ToolsPreset, MINIMUM_CLAUDE_CODE_VERSION, SDK_VERSION,
};
pub use query::{query, query_with_config, query_with_messages, query_with_messages_and_config, QueryHandle};
pub use sdk_mcp::{
    create_sdk_mcp_server, tool, SdkMcpServer, SdkMcpServerConfig, SdkMcpTool, SdkMcpToolHandler,
};
pub use session::{
    build_mirror_batcher, delete_session, delete_session_via_store, fork_session,
    fork_session_via_store, get_session_info, get_session_info_from_store, get_session_messages,
    get_session_messages_from_store, get_subagent_messages, get_subagent_messages_from_store,
    import_session_to_store, list_sessions, list_sessions_from_store, list_subagents,
    list_subagents_from_store, materialize_resume_session, rename_session,
    rename_session_via_store, tag_session, tag_session_via_store, ForkSessionResult,
    InMemorySessionStore, MaterializedResume, SDKSessionInfo, SessionKey, SessionListSubkeysKey,
    SessionMessage, SessionStore, SessionStoreEntry, SessionStoreFlushMode, SessionStoreListEntry,
    SessionSummaryEntry, SharedStore, StoreError, TranscriptMirrorBatcher,
};
pub use transport::{SubprocessCLITransport, Transport};
pub use types::{
    terminal_task_statuses, AssistantMessage, AssistantMessageError, ContentBlock, DeferredToolUse,
    EffortLevel, HookEventMessage, Message, MirrorErrorMessage, PermissionBehavior, PermissionMode,
    PermissionRuleValue, PermissionUpdate, PermissionUpdateDestination, PermissionUpdateType,
    RateLimitEvent, RateLimitInfo, RateLimitStatus, RateLimitType, ResultMessage, ServerToolName,
    ServerToolResultBlock, ServerToolUseBlock, StreamEvent, SystemMessage, SystemMessageView,
    TaskNotificationMessage, TaskNotificationStatus, TaskProgressMessage, TaskStartedMessage,
    TaskUpdatedMessage, TaskUpdatedStatus, TaskUsage, TextBlock, ThinkingBlock, ToolResultBlock,
    ToolUseBlock, UserContent, UserMessage,
};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
