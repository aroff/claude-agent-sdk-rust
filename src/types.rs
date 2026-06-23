//! Type definitions for Claude SDK messages.
//!
//! Rust port of `claude_agent_sdk.types` focused on Claude message
//! structures and content blocks. Each type mirrors the Python dataclass /
//! TypedDict field-for-field so that wire payloads parse identically.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Literal-style enums
// ---------------------------------------------------------------------------

/// Permission update destination (matches TypeScript control protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionUpdateDestination {
    #[serde(rename = "userSettings")]
    UserSettings,
    #[serde(rename = "projectSettings")]
    ProjectSettings,
    #[serde(rename = "localSettings")]
    LocalSettings,
    #[serde(rename = "session")]
    Session,
}

impl PermissionUpdateDestination {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "userSettings" => Self::UserSettings,
            "projectSettings" => Self::ProjectSettings,
            "localSettings" => Self::LocalSettings,
            "session" => Self::Session,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserSettings => "userSettings",
            Self::ProjectSettings => "projectSettings",
            Self::LocalSettings => "localSettings",
            Self::Session => "session",
        }
    }
}

/// Permission behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionBehavior {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

impl PermissionBehavior {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            "ask" => Self::Ask,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

/// Discriminator for a [`PermissionUpdate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionUpdateType {
    AddRules,
    ReplaceRules,
    RemoveRules,
    SetMode,
    AddDirectories,
    RemoveDirectories,
}

impl PermissionUpdateType {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "addRules" => Self::AddRules,
            "replaceRules" => Self::ReplaceRules,
            "removeRules" => Self::RemoveRules,
            "setMode" => Self::SetMode,
            "addDirectories" => Self::AddDirectories,
            "removeDirectories" => Self::RemoveDirectories,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AddRules => "addRules",
            Self::ReplaceRules => "replaceRules",
            Self::RemoveRules => "removeRules",
            Self::SetMode => "setMode",
            Self::AddDirectories => "addDirectories",
            Self::RemoveDirectories => "removeDirectories",
        }
    }
}

/// Permission rule value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRuleValue {
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

/// Permission update configuration (mirrors the TypeScript control protocol).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionUpdate {
    pub r#type: Option<PermissionUpdateType>,
    pub rules: Option<Vec<PermissionRuleValue>>,
    pub behavior: Option<PermissionBehavior>,
    pub mode: Option<PermissionMode>,
    pub directories: Option<Vec<String>>,
    pub destination: Option<PermissionUpdateDestination>,
}

impl PermissionUpdate {
    /// Serialize to the control-protocol dict format (inverse of [`Self::from_dict`]).
    pub fn to_dict(&self) -> Map<String, Value> {
        let mut result = Map::new();
        if let Some(t) = self.r#type {
            result.insert("type".into(), Value::String(t.as_str().into()));
        }

        if let Some(dest) = self.destination {
            result.insert("destination".into(), Value::String(dest.as_str().into()));
        }

        match self.r#type {
            Some(
                PermissionUpdateType::AddRules
                | PermissionUpdateType::ReplaceRules
                | PermissionUpdateType::RemoveRules,
            ) => {
                if let Some(rules) = &self.rules {
                    let arr: Vec<Value> = rules
                        .iter()
                        .map(|r| {
                            let mut m = Map::new();
                            m.insert("toolName".into(), Value::String(r.tool_name.clone()));
                            m.insert(
                                "ruleContent".into(),
                                r.rule_content
                                    .clone()
                                    .map(Value::String)
                                    .unwrap_or(Value::Null),
                            );
                            Value::Object(m)
                        })
                        .collect();
                    result.insert("rules".into(), Value::Array(arr));
                }
                if let Some(b) = self.behavior {
                    result.insert("behavior".into(), Value::String(b.as_str().into()));
                }
            }
            Some(PermissionUpdateType::SetMode) => {
                if let Some(mode) = &self.mode {
                    result.insert("mode".into(), Value::String(mode_to_str(mode).into()));
                }
            }
            Some(
                PermissionUpdateType::AddDirectories | PermissionUpdateType::RemoveDirectories,
            ) => {
                if let Some(dirs) = &self.directories {
                    result.insert(
                        "directories".into(),
                        Value::Array(dirs.iter().cloned().map(Value::String).collect()),
                    );
                }
            }
            None => {}
        }

        result
    }

    /// Construct from the control-protocol dict format (inverse of [`Self::to_dict`]).
    pub fn from_dict(data: &Map<String, Value>) -> Result<Self, String> {
        let r#type = match data.get("type") {
            Some(Value::String(s)) => PermissionUpdateType::from_str_lossy(s)
                .ok_or_else(|| format!("invalid type '{s}'"))?,
            _ => return Err("missing 'type'".into()),
        };

        let mut rules = None;
        if let Some(Value::Array(arr)) = data.get("rules") {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let m = item.as_object().ok_or("rule entry not object")?;
                let tool_name = m
                    .get("toolName")
                    .and_then(Value::as_str)
                    .ok_or("missing toolName")?
                    .to_string();
                let rule_content = match m.get("ruleContent") {
                    Some(Value::String(s)) => Some(s.clone()),
                    _ => None,
                };
                out.push(PermissionRuleValue {
                    tool_name,
                    rule_content,
                });
            }
            rules = Some(out);
        }

        let behavior = data
            .get("behavior")
            .and_then(Value::as_str)
            .and_then(PermissionBehavior::from_str_lossy);
        let mode = data
            .get("mode")
            .and_then(Value::as_str)
            .and_then(mode_from_str);
        let directories = data
            .get("directories")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
        let destination = data
            .get("destination")
            .and_then(Value::as_str)
            .and_then(PermissionUpdateDestination::from_str_lossy);

        Ok(Self {
            r#type: Some(r#type),
            rules,
            behavior,
            mode,
            directories,
            destination,
        })
    }
}

fn mode_to_str(m: &PermissionMode) -> &'static str {
    match m {
        PermissionMode::Default => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Plan => "plan",
        PermissionMode::BypassPermissions => "bypassPermissions",
        PermissionMode::DontAsk => "dontAsk",
        PermissionMode::Auto => "auto",
    }
}

fn mode_from_str(s: &str) -> Option<PermissionMode> {
    Some(match s {
        "default" => PermissionMode::Default,
        "acceptEdits" => PermissionMode::AcceptEdits,
        "plan" => PermissionMode::Plan,
        "bypassPermissions" => PermissionMode::BypassPermissions,
        "dontAsk" => PermissionMode::DontAsk,
        "auto" => PermissionMode::Auto,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Literal-style enums (message enums)
// ---------------------------------------------------------------------------

/// Permission modes supported by the SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    #[serde(rename = "dontAsk")]
    DontAsk,
    #[serde(rename = "auto")]
    Auto,
}

/// Effort levels supported by adaptive thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl EffortLevel {
    /// All valid effort level strings (mirrors `Literal` args in Python).
    pub fn variants() -> &'static [&'static str] {
        &["low", "medium", "high", "xhigh", "max"]
    }
}

/// Server-side tool names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerToolName {
    Advisor,
    WebSearch,
    WebFetch,
    CodeExecution,
    BashCodeExecution,
    TextEditorCodeExecution,
    ToolSearchToolRegex,
    ToolSearchToolBm25,
}

impl ServerToolName {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "advisor" => ServerToolName::Advisor,
            "web_search" => ServerToolName::WebSearch,
            "web_fetch" => ServerToolName::WebFetch,
            "code_execution" => ServerToolName::CodeExecution,
            "bash_code_execution" => ServerToolName::BashCodeExecution,
            "text_editor_code_execution" => ServerToolName::TextEditorCodeExecution,
            "tool_search_tool_regex" => ServerToolName::ToolSearchToolRegex,
            "tool_search_tool_bm25" => ServerToolName::ToolSearchToolBm25,
            _ => return None,
        })
    }
}

/// Error categories reported on an `AssistantMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantMessageError {
    #[serde(rename = "authentication_failed")]
    AuthenticationFailed,
    #[serde(rename = "billing_error")]
    BillingError,
    #[serde(rename = "rate_limit")]
    RateLimit,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "server_error")]
    ServerError,
    #[serde(rename = "unknown")]
    Unknown,
}

impl AssistantMessageError {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "authentication_failed" => Self::AuthenticationFailed,
            "billing_error" => Self::BillingError,
            "rate_limit" => Self::RateLimit,
            "invalid_request" => Self::InvalidRequest,
            "server_error" => Self::ServerError,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

/// Status values reported by a `task_notification` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskNotificationStatus {
    Completed,
    Failed,
    Stopped,
}

impl TaskNotificationStatus {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => return None,
        })
    }
}

/// Status values reported inside a `task_updated` patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskUpdatedStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Killed,
}

impl TaskUpdatedStatus {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "killed" => Self::Killed,
            _ => return None,
        })
    }
}

/// Rate limit status values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitStatus {
    #[serde(rename = "allowed")]
    Allowed,
    #[serde(rename = "allowed_warning")]
    AllowedWarning,
    #[serde(rename = "rejected")]
    Rejected,
}

impl RateLimitStatus {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "allowed" => Self::Allowed,
            "allowed_warning" => Self::AllowedWarning,
            "rejected" => Self::Rejected,
            _ => return None,
        })
    }
}

/// Which rate limit window applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitType {
    #[serde(rename = "five_hour")]
    FiveHour,
    #[serde(rename = "seven_day")]
    SevenDay,
    #[serde(rename = "seven_day_opus")]
    SevenDayOpus,
    #[serde(rename = "seven_day_sonnet")]
    SevenDaySonnet,
    #[serde(rename = "overage")]
    Overage,
}

impl RateLimitType {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        Some(match s {
            "five_hour" => Self::FiveHour,
            "seven_day" => Self::SevenDay,
            "seven_day_opus" => Self::SevenDayOpus,
            "seven_day_sonnet" => Self::SevenDaySonnet,
            "overage" => Self::Overage,
            _ => return None,
        })
    }
}

/// Terminal task status set spanning both lifecycle vocabularies.
///
/// `task_notification` reports `stopped`; `task_updated` reports the raw
/// `killed`. Consumers should treat the `status` of either message the same.
pub fn terminal_task_statuses() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        ["completed", "failed", "stopped", "killed"]
            .into_iter()
            .collect()
    })
}

/// Usage statistics reported in task_progress / task_notification messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUsage {
    pub total_tokens: i64,
    pub tool_uses: i64,
    pub duration_ms: i64,
}

impl TaskUsage {
    pub fn from_value(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        Some(Self {
            total_tokens: obj.get("total_tokens")?.as_i64()?,
            tool_uses: obj.get("tool_uses")?.as_i64()?,
            duration_ms: obj.get("duration_ms")?.as_i64()?,
        })
    }
}

// ---------------------------------------------------------------------------
// Content blocks
// ---------------------------------------------------------------------------

/// Text content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

/// Thinking content block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub thinking: String,
    pub signature: String,
}

/// Tool use content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: Map<String, Value>,
}

/// Tool result content block.
///
/// `content` mirrors the Python union `str | list[dict[str, Any]] | None` and
/// is therefore carried as an opaque JSON value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, rename = "is_error", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Server-side tool use block (e.g. advisor, web_search).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerToolUseBlock {
    pub id: String,
    pub name: ServerToolName,
    pub input: Map<String, Value>,
}

/// Result block returned for a server-side tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerToolResultBlock {
    pub tool_use_id: String,
    pub content: Map<String, Value>,
}

/// Discriminated union of all content blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text(TextBlock),
    #[serde(rename = "thinking")]
    Thinking(ThinkingBlock),
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseBlock),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultBlock),
    #[serde(rename = "server_tool_use")]
    ServerToolUse(ServerToolUseBlock),
    #[serde(rename = "advisor_tool_result")]
    ServerToolResult(ServerToolResultBlock),
}

impl ContentBlock {
    pub fn as_text(&self) -> Option<&TextBlock> {
        match self {
            ContentBlock::Text(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_thinking(&self) -> Option<&ThinkingBlock> {
        match self {
            ContentBlock::Thinking(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_tool_use(&self) -> Option<&ToolUseBlock> {
        match self {
            ContentBlock::ToolUse(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_tool_result(&self) -> Option<&ToolResultBlock> {
        match self {
            ContentBlock::ToolResult(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_server_tool_use(&self) -> Option<&ServerToolUseBlock> {
        match self {
            ContentBlock::ServerToolUse(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_server_tool_result(&self) -> Option<&ServerToolResultBlock> {
        match self {
            ContentBlock::ServerToolResult(b) => Some(b),
            _ => None,
        }
    }
}

/// A tool use that was deferred by a PreToolUse hook returning "defer".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeferredToolUse {
    pub id: String,
    pub name: String,
    pub input: Map<String, Value>,
}

/// Either a plain string or a list of content blocks (UserMessage.content).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

// ---------------------------------------------------------------------------
// Message structs
// ---------------------------------------------------------------------------

/// User message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: UserContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<Value>,
}

/// Assistant message with content blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AssistantMessageError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    #[serde(
        default,
        rename = "message_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// Base system message carrying raw metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemMessage {
    pub subtype: String,
    pub data: Value,
}

/// System message emitted when a task starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskStartedMessage {
    pub subtype: String,
    pub data: Value,
    pub task_id: String,
    pub description: String,
    pub uuid: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
}

/// System message emitted while a task is in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskProgressMessage {
    pub subtype: String,
    pub data: Value,
    pub task_id: String,
    pub description: String,
    pub usage: TaskUsage,
    pub uuid: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
}

/// System message emitted when a task completes, fails, or is stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskNotificationMessage {
    pub subtype: String,
    pub data: Value,
    pub task_id: String,
    pub status: TaskNotificationStatus,
    pub output_file: String,
    pub summary: String,
    pub uuid: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TaskUsage>,
}

/// System message emitted when a background task's state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskUpdatedMessage {
    pub subtype: String,
    pub data: Value,
    pub task_id: String,
    pub patch: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskUpdatedStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// System message emitted when a `SessionStore::append` call fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirrorErrorMessage {
    pub subtype: String,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Value>,
    pub error: String,
}

/// Hook lifecycle event (system/hook_started or system/hook_response).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEventMessage {
    pub subtype: String,
    pub data: Value,
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// Final result message with cost and usage information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultMessage {
    pub subtype: String,
    pub duration_ms: i64,
    pub duration_api_ms: i64,
    pub is_error: bool,
    pub num_turns: i64,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
    #[serde(
        default,
        rename = "modelUsage",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_denials: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred_tool_use: Option<DeferredToolUse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_error_status: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// Stream event for partial message updates during streaming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEvent {
    pub uuid: String,
    pub session_id: String,
    pub event: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Rate limit status emitted by the CLI when rate limit state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub status: RateLimitStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_type: Option<RateLimitType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overage_status: Option<RateLimitStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overage_resets_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overage_disabled_reason: Option<String>,
    pub raw: Value,
}

/// Rate limit event emitted when rate limit info changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitEvent {
    pub rate_limit_info: RateLimitInfo,
    pub uuid: String,
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// Top-level message enum
// ---------------------------------------------------------------------------

/// All message variants produced by the parser.
///
/// System-message subclasses (`TaskStartedMessage`, `HookEventMessage`, etc.)
/// carry their own base `subtype`/`data` fields so legacy code paths that
/// treat them as `SystemMessage` keep working — see [`Message::as_system`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    TaskStarted(TaskStartedMessage),
    TaskProgress(TaskProgressMessage),
    TaskNotification(TaskNotificationMessage),
    TaskUpdated(TaskUpdatedMessage),
    MirrorError(MirrorErrorMessage),
    HookEvent(HookEventMessage),
    Result(ResultMessage),
    StreamEvent(StreamEvent),
    RateLimitEvent(RateLimitEvent),
}

/// Read-only view onto the base `SystemMessage` fields shared by every
/// system-family variant. Returned by [`Message::as_system`].
#[derive(Debug, Clone, Copy)]
pub struct SystemMessageView<'a> {
    pub subtype: &'a str,
    pub data: &'a Value,
}

impl Message {
    /// True for every system-family variant (mirrors `isinstance(msg, SystemMessage)`).
    pub fn is_system(&self) -> bool {
        matches!(
            self,
            Message::System(_)
                | Message::TaskStarted(_)
                | Message::TaskProgress(_)
                | Message::TaskNotification(_)
                | Message::TaskUpdated(_)
                | Message::MirrorError(_)
                | Message::HookEvent(_)
        )
    }

    /// Base `SystemMessage` view if this message is part of the system family.
    pub fn as_system(&self) -> Option<SystemMessageView<'_>> {
        let (subtype, data) = match self {
            Message::System(m) => (m.subtype.as_str(), &m.data),
            Message::TaskStarted(m) => (m.subtype.as_str(), &m.data),
            Message::TaskProgress(m) => (m.subtype.as_str(), &m.data),
            Message::TaskNotification(m) => (m.subtype.as_str(), &m.data),
            Message::TaskUpdated(m) => (m.subtype.as_str(), &m.data),
            Message::MirrorError(m) => (m.subtype.as_str(), &m.data),
            Message::HookEvent(m) => (m.subtype.as_str(), &m.data),
            _ => return None,
        };
        Some(SystemMessageView { subtype, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_level_variants_match_python() {
        assert_eq!(
            EffortLevel::variants(),
            &["low", "medium", "high", "xhigh", "max"]
        );
    }

    #[test]
    fn terminal_task_statuses_set() {
        let set = terminal_task_statuses();
        for s in ["completed", "failed", "stopped", "killed"] {
            assert!(set.contains(s), "missing {s}");
        }
        assert!(!set.contains("running"));
    }

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
            content: vec![ContentBlock::Text(TextBlock {
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
            content: vec![ContentBlock::Thinking(block)],
            model: "claude-opus-4-1-20250805".into(),
            parent_tool_use_id: None,
            error: None,
            usage: None,
            message_id: None,
            stop_reason: None,
            session_id: None,
            uuid: None,
        };
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
            input: serde_json::json!({"file_path": "/test.txt"})
                .as_object()
                .cloned()
                .unwrap(),
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

    #[test]
    fn server_tool_name_round_trip() {
        assert_eq!(
            ServerToolName::from_str_lossy("advisor"),
            Some(ServerToolName::Advisor)
        );
        assert!(ServerToolName::from_str_lossy("nope").is_none());
    }

    #[test]
    fn assistant_message_error_round_trip() {
        assert_eq!(
            AssistantMessageError::from_str_lossy("rate_limit"),
            Some(AssistantMessageError::RateLimit)
        );
        assert_eq!(
            AssistantMessageError::from_str_lossy("unknown"),
            Some(AssistantMessageError::Unknown)
        );
    }

    #[test]
    fn rate_limit_status_round_trip() {
        assert_eq!(
            RateLimitStatus::from_str_lossy("allowed_warning"),
            Some(RateLimitStatus::AllowedWarning)
        );
    }

    #[test]
    fn rate_limit_type_round_trip() {
        assert_eq!(
            RateLimitType::from_str_lossy("seven_day_opus"),
            Some(RateLimitType::SevenDayOpus)
        );
    }
}
