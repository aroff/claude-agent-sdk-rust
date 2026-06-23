//! Query options for the Claude Agent SDK.
//!
//! Rust port of the Python `ClaudeAgentOptions` dataclass. Holds every
//! configuration knob the CLI accepts; [`ClaudeAgentOptions::build_command`]
//! turns it into the `claude` argv vector that the subprocess transport
//! spawns.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::session::{SessionStore, SessionStoreFlushMode};
use crate::types::PermissionMode;

/// Minimum supported Claude Code CLI version.
pub const MINIMUM_CLAUDE_CODE_VERSION: &str = "2.0.0";

/// SDK version reported to the CLI via `CLAUDE_AGENT_SDK_VERSION`.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Preset selector for the base set of built-in tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolsPreset {
    /// `{"type": "preset", "preset": "claude_code"}` — use the default Claude
    /// Code tool set. Maps to `--tools default` on the wire.
    #[serde(rename = "preset")]
    Preset { preset: String },
}

/// Preset selector for the system prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemPromptPreset {
    #[serde(rename = "preset")]
    Preset {
        preset: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<String>,
        #[serde(
            default,
            rename = "exclude_dynamic_sections",
            skip_serializing_if = "Option::is_none"
        )]
        exclude_dynamic_sections: Option<bool>,
    },
}

/// System prompt loaded from a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemPromptFile {
    #[serde(rename = "file")]
    File { path: String },
}

/// All accepted shapes for the `system_prompt` option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemPrompt {
    Custom(String),
    Preset(SystemPromptPreset),
    File(SystemPromptFile),
}

/// All accepted shapes for the `tools` option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tools {
    List(Vec<String>),
    Preset(ToolsPreset),
}

/// Sandbox settings (passed through verbatim into the merged settings JSON).
pub type SandboxSettings = Map<String, Value>;

/// Plugin configuration; only local plugins are supported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdkPluginConfig {
    #[serde(rename = "type")]
    pub plugin_type: String,
    pub path: String,
}

/// MCP server configuration entry (opaque — passed through to the CLI).
pub type McpServerConfig = Map<String, Value>;

/// Thinking configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    Adaptive {
        #[serde(default, rename = "display", skip_serializing_if = "Option::is_none")]
        display: Option<String>,
    },
    Enabled {
        budget_tokens: i64,
        #[serde(default, rename = "display", skip_serializing_if = "Option::is_none")]
        display: Option<String>,
    },
    Disabled,
}

/// Effort level for adaptive thinking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Effort {
    Named(String), // "low" | "medium" | "high" | "xhigh" | "max"
    Tokens(i64),
}

/// Task budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBudget {
    pub total: i64,
}

/// Query options for the Claude SDK.
///
/// Field names mirror the Python dataclass; the [`Serialize`] impl is not
/// used for the wire format (CLI args are built by [`Self::build_command`]).
#[derive(Clone)]
pub struct ClaudeAgentOptions {
    pub tools: Option<Tools>,
    pub allowed_tools: Vec<String>,
    pub system_prompt: Option<SystemPrompt>,
    pub mcp_servers: McpServers,
    pub strict_mcp_config: bool,
    pub permission_mode: Option<PermissionMode>,
    pub continue_conversation: bool,
    pub resume: Option<String>,
    pub session_id: Option<String>,
    pub max_turns: Option<i64>,
    pub max_budget_usd: Option<f64>,
    pub disallowed_tools: Vec<String>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub betas: Vec<String>,
    pub permission_prompt_tool_name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub cli_path: Option<PathBuf>,
    pub settings: Option<String>,
    pub add_dirs: Vec<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub extra_args: BTreeMap<String, Option<String>>,
    pub max_buffer_size: Option<usize>,
    pub stderr: Option<StderrCallback>,
    pub include_partial_messages: bool,
    pub include_hook_events: bool,
    pub fork_session: bool,
    pub setting_sources: Option<Vec<String>>,
    pub skills: Option<SkillsFilter>,
    pub sandbox: Option<SandboxSettings>,
    pub plugins: Vec<SdkPluginConfig>,
    pub max_thinking_tokens: Option<i64>,
    pub thinking: Option<ThinkingConfig>,
    pub effort: Option<Effort>,
    pub output_format: Option<Value>,
    pub user: Option<String>,
    pub enable_file_checkpointing: bool,
    pub session_store: Option<Arc<dyn SessionStore>>,
    pub session_store_flush: SessionStoreFlushMode,
    pub load_timeout_ms: u64,
    pub task_budget: Option<TaskBudget>,
}

impl Default for ClaudeAgentOptions {
    fn default() -> Self {
        Self {
            tools: None,
            allowed_tools: Vec::new(),
            system_prompt: None,
            mcp_servers: McpServers::None,
            strict_mcp_config: false,
            permission_mode: None,
            continue_conversation: false,
            resume: None,
            session_id: None,
            max_turns: None,
            max_budget_usd: None,
            disallowed_tools: Vec::new(),
            model: None,
            fallback_model: None,
            betas: Vec::new(),
            permission_prompt_tool_name: None,
            cwd: None,
            cli_path: None,
            settings: None,
            add_dirs: Vec::new(),
            env: BTreeMap::new(),
            extra_args: BTreeMap::new(),
            max_buffer_size: None,
            stderr: None,
            include_partial_messages: false,
            include_hook_events: false,
            fork_session: false,
            setting_sources: None,
            skills: None,
            sandbox: None,
            plugins: Vec::new(),
            max_thinking_tokens: None,
            thinking: None,
            effort: None,
            output_format: None,
            user: None,
            enable_file_checkpointing: false,
            session_store: None,
            session_store_flush: SessionStoreFlushMode::Batched,
            load_timeout_ms: 60_000,
            task_budget: None,
        }
    }
}

/// Allowed values for the `skills` option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsFilter {
    All,
    List(Vec<String>),
}

/// Accepted shapes for `mcp_servers`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum McpServers {
    #[default]
    None,
    Map(BTreeMap<String, McpServerConfig>),
    Path(String),
}

/// Callback for stderr lines. Stored as a boxed function so callers can log
/// or react to CLI diagnostic output.
pub type StderrCallback = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

impl std::fmt::Debug for ClaudeAgentOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeAgentOptions")
            .field("model", &self.model)
            .field("permission_mode", &self.permission_mode)
            .field("cwd", &self.cwd)
            .field("stderr_set", &self.stderr.is_some())
            .finish_non_exhaustive()
    }
}

impl ClaudeAgentOptions {
    /// Build the CLI argv vector (without the binary path). Mirrors Python's
    /// `SubprocessCLITransport._build_command` field-for-field.
    pub fn build_command(&self) -> Vec<String> {
        let mut cmd = vec![
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
        ];

        // system_prompt
        match &self.system_prompt {
            None => {
                cmd.push("--system-prompt".into());
                cmd.push(String::new());
            }
            Some(SystemPrompt::Custom(s)) => {
                cmd.push("--system-prompt".into());
                cmd.push(s.clone());
            }
            Some(SystemPrompt::Preset(p)) => {
                if let SystemPromptPreset::Preset {
                    append: Some(a), ..
                } = p
                {
                    cmd.push("--append-system-prompt".into());
                    cmd.push(a.clone());
                }
            }
            Some(SystemPrompt::File(f)) => match f {
                SystemPromptFile::File { path } => {
                    cmd.push("--system-prompt-file".into());
                    cmd.push(path.clone());
                }
            },
        }

        // tools (base set)
        if let Some(tools) = &self.tools {
            match tools {
                Tools::List(list) => {
                    if list.is_empty() {
                        cmd.push("--tools".into());
                        cmd.push(String::new());
                    } else {
                        cmd.push("--tools".into());
                        cmd.push(list.join(","));
                    }
                }
                Tools::Preset(_) => {
                    cmd.push("--tools".into());
                    cmd.push("default".into());
                }
            }
        }

        let (effective_allowed_tools, effective_setting_sources) = self.apply_skills_defaults();

        if !effective_allowed_tools.is_empty() {
            cmd.push("--allowedTools".into());
            cmd.push(effective_allowed_tools.join(","));
        }

        if let Some(n) = self.max_turns {
            cmd.push("--max-turns".into());
            cmd.push(n.to_string());
        }
        if let Some(b) = self.max_budget_usd {
            cmd.push("--max-budget-usd".into());
            cmd.push(b.to_string());
        }
        if !self.disallowed_tools.is_empty() {
            cmd.push("--disallowedTools".into());
            cmd.push(self.disallowed_tools.join(","));
        }
        if let Some(tb) = self.task_budget.as_ref() {
            cmd.push("--task-budget".into());
            cmd.push(tb.total.to_string());
        }
        if let Some(m) = &self.model {
            cmd.push("--model".into());
            cmd.push(m.clone());
        }
        if let Some(m) = &self.fallback_model {
            cmd.push("--fallback-model".into());
            cmd.push(m.clone());
        }
        if !self.betas.is_empty() {
            cmd.push("--betas".into());
            cmd.push(self.betas.join(","));
        }
        if let Some(t) = &self.permission_prompt_tool_name {
            cmd.push("--permission-prompt-tool".into());
            cmd.push(t.clone());
        }
        if let Some(mode) = &self.permission_mode {
            cmd.push("--permission-mode".into());
            cmd.push(permission_mode_str(mode).into());
        }
        if self.continue_conversation {
            cmd.push("--continue".into());
        }
        if let Some(r) = &self.resume {
            cmd.push("--resume".into());
            cmd.push(r.clone());
        }
        if let Some(s) = &self.session_id {
            cmd.push("--session-id".into());
            cmd.push(s.clone());
        }

        // settings + sandbox merge
        if let Some(value) = self.build_settings_value() {
            cmd.push("--settings".into());
            cmd.push(value);
        }

        for dir in &self.add_dirs {
            cmd.push("--add-dir".into());
            cmd.push(dir.to_string_lossy().into_owned());
        }

        // mcp config
        match &self.mcp_servers {
            McpServers::Map(map) => {
                if !map.is_empty() {
                    let mut servers_for_cli = Map::new();
                    for (name, config) in map {
                        if config.get("type").and_then(Value::as_str) == Some("sdk") {
                            let mut stripped = config.clone();
                            stripped.remove("instance");
                            servers_for_cli.insert(name.clone(), Value::Object(stripped));
                        } else {
                            servers_for_cli.insert(name.clone(), Value::Object(config.clone()));
                        }
                    }
                    let mut mcp = Map::new();
                    mcp.insert("mcpServers".into(), Value::Object(servers_for_cli));
                    cmd.push("--mcp-config".into());
                    cmd.push(Value::Object(mcp).to_string());
                }
            }
            McpServers::Path(p) => {
                cmd.push("--mcp-config".into());
                cmd.push(p.clone());
            }
            McpServers::None => {}
        }

        if self.include_partial_messages {
            cmd.push("--include-partial-messages".into());
        }
        if self.include_hook_events {
            cmd.push("--include-hook-events".into());
        }
        if self.strict_mcp_config {
            cmd.push("--strict-mcp-config".into());
        }
        if self.fork_session {
            cmd.push("--fork-session".into());
        }
        if self.session_store.is_some() {
            cmd.push("--session-mirror".into());
        }
        if self.enable_file_checkpointing {
            cmd.push("--enable-file-checkpointing".into());
        }

        if let Some(sources) = &effective_setting_sources {
            cmd.push(format!("--setting-sources={}", sources.join(",")));
        }

        for plugin in &self.plugins {
            if plugin.plugin_type == "local" {
                cmd.push("--plugin-dir".into());
                cmd.push(plugin.path.clone());
            }
        }

        for (flag, value) in &self.extra_args {
            match value {
                None => cmd.push(format!("--{flag}")),
                Some(v) => {
                    cmd.push(format!("--{flag}"));
                    cmd.push(v.clone());
                }
            }
        }

        // thinking
        if let Some(t) = &self.thinking {
            match t {
                ThinkingConfig::Adaptive { .. } => {
                    cmd.push("--thinking".into());
                    cmd.push("adaptive".into());
                }
                ThinkingConfig::Enabled { budget_tokens, .. } => {
                    cmd.push("--max-thinking-tokens".into());
                    cmd.push(budget_tokens.to_string());
                }
                ThinkingConfig::Disabled => {
                    cmd.push("--thinking".into());
                    cmd.push("disabled".into());
                }
            }
            if let ThinkingConfig::Adaptive { display: Some(d) }
            | ThinkingConfig::Enabled {
                display: Some(d), ..
            } = t
            {
                cmd.push("--thinking-display".into());
                cmd.push(d.clone());
            }
        } else if let Some(tokens) = self.max_thinking_tokens {
            cmd.push("--max-thinking-tokens".into());
            cmd.push(tokens.to_string());
        }

        if let Some(e) = &self.effort {
            match e {
                Effort::Named(s) => {
                    cmd.push("--effort".into());
                    cmd.push(s.clone());
                }
                Effort::Tokens(n) => {
                    cmd.push("--effort".into());
                    cmd.push(n.to_string());
                }
            }
        }

        // output_format (json_schema)
        if let Some(Value::Object(obj)) = &self.output_format {
            if obj.get("type").and_then(Value::as_str) == Some("json_schema") {
                if let Some(schema) = obj.get("schema") {
                    cmd.push("--json-schema".into());
                    cmd.push(schema.to_string());
                }
            }
        }

        // Always streaming input (matches TypeScript/Python SDK).
        cmd.push("--input-format".into());
        cmd.push("stream-json".into());

        cmd
    }

    /// Compute effective `allowed_tools` and `setting_sources` after applying
    /// the `skills` option's defaults. Mirrors `_apply_skills_defaults`.
    pub fn apply_skills_defaults(&self) -> (Vec<String>, Option<Vec<String>>) {
        let mut allowed: Vec<String> = self.allowed_tools.clone();
        let mut sources: Option<Vec<String>> = self.setting_sources.clone();

        match &self.skills {
            None => {}
            Some(SkillsFilter::All) => {
                if !allowed.iter().any(|t| t == "Skill") {
                    allowed.push("Skill".into());
                }
                if sources.is_none() {
                    sources = Some(vec!["user".into(), "project".into()]);
                }
            }
            Some(SkillsFilter::List(names)) => {
                for name in names {
                    let pattern = format!("Skill({name})");
                    if !allowed.contains(&pattern) {
                        allowed.push(pattern);
                    }
                }
                if sources.is_none() {
                    sources = Some(vec!["user".into(), "project".into()]);
                }
            }
        }
        (allowed, sources)
    }

    /// Build the merged settings value (`--settings` payload), handling the
    /// sandbox merge. Returns `None` when neither settings nor sandbox is set.
    pub fn build_settings_value(&self) -> Option<String> {
        let has_settings = self.settings.is_some();
        let has_sandbox = self.sandbox.is_some();
        if !has_settings && !has_sandbox {
            return None;
        }
        if has_settings && !has_sandbox {
            return self.settings.clone();
        }
        let mut obj: Map<String, Value> = Map::new();
        if let Some(s) = &self.settings {
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(trimmed) {
                    obj = m;
                }
            }
        }
        if let Some(sandbox) = &self.sandbox {
            obj.insert("sandbox".into(), Value::Object(sandbox.clone()));
        }
        Some(Value::Object(obj).to_string())
    }

    /// Build the environment for the subprocess, including SDK defaults.
    pub fn build_env(&self) -> BTreeMap<String, String> {
        let mut env: BTreeMap<String, String> = std::env::vars()
            .filter(|(k, _)| k != "CLAUDECODE")
            .collect();
        env.insert("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-rust".into());
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        env.insert("CLAUDE_AGENT_SDK_VERSION".into(), SDK_VERSION.into());
        if let Some(cwd) = &self.cwd {
            env.insert("PWD".into(), cwd.to_string_lossy().into_owned());
        }
        env
    }
}

/// Convert a [`PermissionMode`] to its CLI string form.
pub fn permission_mode_str(m: &PermissionMode) -> &'static str {
    match m {
        PermissionMode::Default => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Plan => "plan",
        PermissionMode::BypassPermissions => "bypassPermissions",
        PermissionMode::DontAsk => "dontAsk",
        PermissionMode::Auto => "auto",
    }
}

/// Compare two CLI version strings of the form `MAJOR.MINOR.PATCH`.
/// Returns `Ordering::Less` when `a` is older than `b`.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<i64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<i64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    std::cmp::Ordering::Equal
}

// Placeholder reference for the optional task_budget field. The Python
// dataclass carries it directly; here it lives on ClaudeAgentOptions via
// the public `task_budget: Option<TaskBudget>` field declared above.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_builds_minimal_command() {
        let opts = ClaudeAgentOptions::default();
        let cmd = opts.build_command();
        // Always-present flags
        assert!(cmd.contains(&"--output-format".to_string()));
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "stream-json"));
        assert!(cmd.contains(&"--verbose".to_string()));
        // Default empty system prompt
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--system-prompt" && w[1].is_empty()));
        // Streaming input always set
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--input-format" && w[1] == "stream-json"));
    }

    #[test]
    fn session_store_enables_session_mirror_flag() {
        let opts = ClaudeAgentOptions {
            session_store: Some(std::sync::Arc::new(
                crate::session::InMemorySessionStore::new(),
            )),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd.contains(&"--session-mirror".to_string()));
    }

    #[test]
    fn file_checkpointing_enables_cli_flag() {
        let opts = ClaudeAgentOptions {
            enable_file_checkpointing: true,
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd.contains(&"--enable-file-checkpointing".to_string()));
    }

    #[test]
    fn permission_mode_serializes_to_cli_string() {
        let opts = ClaudeAgentOptions {
            permission_mode: Some(PermissionMode::BypassPermissions),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--permission-mode" && w[1] == "bypassPermissions"));
    }

    #[test]
    fn tools_list_joins_with_comma() {
        let opts = ClaudeAgentOptions {
            tools: Some(Tools::List(vec!["Read".into(), "Bash".into()])),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--tools" && w[1] == "Read,Bash"));
    }

    #[test]
    fn tools_empty_list_disables_all() {
        let opts = ClaudeAgentOptions {
            tools: Some(Tools::List(vec![])),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()));
    }

    #[test]
    fn tools_preset_maps_to_default() {
        let opts = ClaudeAgentOptions {
            tools: Some(Tools::Preset(ToolsPreset::Preset {
                preset: "claude_code".into(),
            })),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--tools" && w[1] == "default"));
    }

    #[test]
    fn skills_all_injects_skill_and_setting_sources() {
        let opts = ClaudeAgentOptions {
            skills: Some(SkillsFilter::All),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--allowedTools" && w[1].contains("Skill")));
        assert!(cmd
            .iter()
            .any(|a| a.starts_with("--setting-sources=user,project")));
    }

    #[test]
    fn skills_list_injects_named_skill_patterns() {
        let opts = ClaudeAgentOptions {
            skills: Some(SkillsFilter::List(vec!["alpha".into(), "beta".into()])),
            ..Default::default()
        };
        let (allowed, _) = opts.apply_skills_defaults();
        assert!(allowed.contains(&"Skill(alpha)".to_string()));
        assert!(allowed.contains(&"Skill(beta)".to_string()));
    }

    #[test]
    fn settings_value_passthrough_when_no_sandbox() {
        let opts = ClaudeAgentOptions {
            settings: Some("/path/to/settings.json".into()),
            ..Default::default()
        };
        assert_eq!(
            opts.build_settings_value().as_deref(),
            Some("/path/to/settings.json")
        );
    }

    #[test]
    fn settings_value_merges_sandbox() {
        let mut sandbox = Map::new();
        sandbox.insert("enabled".into(), Value::Bool(true));
        let opts = ClaudeAgentOptions {
            sandbox: Some(sandbox.clone()),
            ..Default::default()
        };
        let v = opts.build_settings_value().unwrap();
        let parsed: Value = serde_json::from_str(&v).unwrap();
        assert_eq!(
            parsed.get("sandbox").unwrap().get("enabled"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn mcp_servers_map_serializes_to_mcp_config() {
        let mut map = BTreeMap::new();
        let mut srv = Map::new();
        srv.insert("type".into(), Value::String("http".into()));
        srv.insert("url".into(), Value::String("https://example.com".into()));
        map.insert("my-server".into(), srv);
        let opts = ClaudeAgentOptions {
            mcp_servers: McpServers::Map(map),
            ..Default::default()
        };
        let cmd = opts.build_command();
        let mcp_idx = cmd.iter().position(|a| a == "--mcp-config").unwrap();
        let payload: Value = serde_json::from_str(&cmd[mcp_idx + 1]).unwrap();
        assert_eq!(
            payload["mcpServers"]["my-server"]["url"],
            "https://example.com"
        );
    }

    #[test]
    fn mcp_servers_strips_instance_from_sdk_type() {
        let mut map = BTreeMap::new();
        let mut srv = Map::new();
        srv.insert("type".into(), Value::String("sdk".into()));
        srv.insert(
            "instance".into(),
            Value::String("should-be-stripped".into()),
        );
        map.insert("sdk-server".into(), srv);
        let opts = ClaudeAgentOptions {
            mcp_servers: McpServers::Map(map),
            ..Default::default()
        };
        let cmd = opts.build_command();
        let mcp_idx = cmd.iter().position(|a| a == "--mcp-config").unwrap();
        let payload: Value = serde_json::from_str(&cmd[mcp_idx + 1]).unwrap();
        assert!(payload["mcpServers"]["sdk-server"]
            .get("instance")
            .is_none());
        assert_eq!(payload["mcpServers"]["sdk-server"]["type"], "sdk");
    }

    #[test]
    fn thinking_adaptive_emits_correct_flags() {
        let opts = ClaudeAgentOptions {
            thinking: Some(ThinkingConfig::Adaptive {
                display: Some("summarized".into()),
            }),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--thinking" && w[1] == "adaptive"));
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--thinking-display" && w[1] == "summarized"));
    }

    #[test]
    fn thinking_enabled_uses_max_thinking_tokens() {
        let opts = ClaudeAgentOptions {
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: 4096,
                display: None,
            }),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--max-thinking-tokens" && w[1] == "4096"));
    }

    #[test]
    fn effort_named_level_emitted() {
        let opts = ClaudeAgentOptions {
            effort: Some(Effort::Named("high".into())),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd.windows(2).any(|w| w[0] == "--effort" && w[1] == "high"));
    }

    #[test]
    fn effort_token_count_emitted() {
        let opts = ClaudeAgentOptions {
            effort: Some(Effort::Tokens(32000)),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--effort" && w[1] == "32000"));
    }

    #[test]
    fn output_format_json_schema_emits_json_schema_flag() {
        let schema = serde_json::json!({"type": "object", "properties": {}});
        let opts = ClaudeAgentOptions {
            output_format: Some(serde_json::json!({
                "type": "json_schema",
                "schema": schema,
            })),
            ..Default::default()
        };
        let cmd = opts.build_command();
        let idx = cmd.iter().position(|a| a == "--json-schema").unwrap();
        let parsed: Value = serde_json::from_str(&cmd[idx + 1]).unwrap();
        assert_eq!(parsed["type"], "object");
    }

    #[test]
    fn extra_args_boolean_flag_and_value() {
        let mut extra = BTreeMap::new();
        extra.insert("dry-run".into(), None);
        extra.insert("label".into(), Some("prod".into()));
        let opts = ClaudeAgentOptions {
            extra_args: extra,
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd.contains(&"--dry-run".to_string()));
        assert!(cmd.windows(2).any(|w| w[0] == "--label" && w[1] == "prod"));
    }

    #[test]
    fn compare_versions_orders_correctly() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("2.0.0", "2.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.9.9", "2.0.0"), Ordering::Less);
        assert_eq!(compare_versions("2.1.0", "2.0.5"), Ordering::Greater);
        assert_eq!(compare_versions("2.1", "2.1.0"), Ordering::Equal);
    }

    #[test]
    fn build_env_filters_claudecode_and_sets_entrypoint() {
        let opts = ClaudeAgentOptions::default();
        let env = opts.build_env();
        assert!(!env.contains_key("CLAUDECODE"));
        assert_eq!(
            env.get("CLAUDE_CODE_ENTRYPOINT").map(String::as_str),
            Some("sdk-rust")
        );
        assert!(env.contains_key("CLAUDE_AGENT_SDK_VERSION"));
    }

    #[test]
    fn resume_session_continue_flags() {
        let opts = ClaudeAgentOptions {
            continue_conversation: true,
            ..Default::default()
        };
        assert!(opts.build_command().contains(&"--continue".to_string()));

        let opts = ClaudeAgentOptions {
            resume: Some("session-123".into()),
            ..Default::default()
        };
        let cmd = opts.build_command();
        assert!(cmd
            .windows(2)
            .any(|w| w[0] == "--resume" && w[1] == "session-123"));
    }
}
