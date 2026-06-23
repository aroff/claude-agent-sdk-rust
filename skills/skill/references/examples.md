# Examples Reference

Runnable examples under `examples/`. Each is a self-contained binary that
compiles with `cargo build --examples` and runs with `cargo run --example <name>`.
All examples require the Claude Code CLI and `ANTHROPIC_API_KEY`.

## quick_start

**Command:** `cargo run --example quick_start`

One-shot `query()` with and without options.

Key APIs: `query()`, `ClaudeAgentOptions`, `SystemPrompt::Custom`,
`Message::Assistant`, `Message::Result`, `block.as_text()`.

## multi_turn

**Command:** `cargo run --example multi_turn`

Multi-turn conversation keeping a single subprocess alive across turns.

Key APIs: `ClaudeSDKClient::new()`, `client.connect()`, `client.query()`,
`client.receive_message()`, `client.disconnect()`.

## sdk_mcp_calculator

**Command:** `cargo run --example sdk_mcp_calculator`

In-process MCP tools implemented as Rust async functions — no separate
server process needed.

Key APIs: `tool()`, `create_sdk_mcp_server()`, `SdkMcpServerConfig`,
`QueryConfig::with_sdk_mcp_server()`, `McpServers::Map`,
`ClaudeSDKClient::with_config()`, `ClaudeAgentOptions::allowed_tools`.

Wiring pattern: `server.config` goes into `ClaudeAgentOptions.mcp_servers`;
`server.handler` is registered via `QueryConfig::with_sdk_mcp_server()`.

## stream_input

**Command:** `cargo run --example stream_input`

Supply messages as a `futures::Stream` instead of a single string.

Key APIs: `query_with_messages()`, `futures::stream::iter()`.

## system_prompt

**Command:** `cargo run --example system_prompt`

Three system prompt variants in a single example.

Key APIs: `SystemPrompt::Custom(String)`,
`SystemPrompt::Preset(SystemPromptPreset::Preset { preset, append, .. })`.

## tools_option

**Command:** `cargo run --example tools_option`

Restrict or expand the built-in tool set, and read the active list from the
`System("init")` message.

Key APIs: `Tools::List(vec![...])`, `Tools::List(vec![])` (disable all),
`Tools::Preset(ToolsPreset::Preset { preset })`.

Init message inspection: `Message::System(s) if s.subtype == "init"` →
`s.data["tools"]` is a JSON array of plain strings.

## max_budget_usd

**Command:** `cargo run --example max_budget_usd`

Hard spend limit; detects budget-exceeded result.

Key APIs: `ClaudeAgentOptions::max_budget_usd`,
`ResultMessage::subtype == "error_max_budget_usd"`.

## include_partial_messages

**Command:** `cargo run --example include_partial_messages`

Receive incremental text deltas as Claude generates its response.

Key APIs: `ClaudeAgentOptions::include_partial_messages`,
`Message::StreamEvent(ev)` → `ev.event["delta"]["text"]`.

## setting_sources

**Command:** `cargo run --example setting_sources`

Control which config directories Claude Code reads (user, project, local).

Key APIs: `ClaudeAgentOptions::setting_sources`:
- `None` — CLI defaults (user + project + local)
- `Some(vec![])` — disable all filesystem sources
- `Some(vec!["user".into()])` — user only
- `Some(vec!["user".into(), "project".into()])` — user + project

Active slash commands are visible in `System("init")` →
`s.data["slash_commands"]`.

## stderr_callback

**Command:** `cargo run --example stderr_callback`

Capture subprocess stderr without mixing it into the structured message stream.

Key APIs: `ClaudeAgentOptions::stderr`,
`StderrCallback = Arc<dyn Fn(&str) + Send + Sync>`.
