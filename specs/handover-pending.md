# Handover Pending List

Status: complete (2026-06-23)

This spec captures the remaining work for the Claude Agent SDK port so the next developer can continue without re-scanning the repo.

## Pending Work

1. **Python async input parity**
   - Port the remaining `query()` ergonomics for streaming/async-iterable prompt input.
   - Keep the current one-shot path working unchanged.
   - Verify the client can still accept incremental input without breaking existing callers.

2. **SDK MCP builder parity**
   - Finish the higher-level MCP helpers that exist in Python:
     - `SdkMcpTool`
     - `tool(...)`
     - `create_sdk_mcp_server(...)`
   - Preserve the current explicit JSON Schema path where needed, but add the convenience layer used by Python callers.
   - Confirm tool input/output normalization matches the SDK behavior expected by the tests.

3. **Process-exit child tracking parity**
   - Match the Python/TypeScript cleanup behavior for subprocesses spawned by the SDK.
   - Ensure child processes are tracked and reaped consistently on normal exit and failure paths.
   - Validate this with an integration test that exercises client shutdown.

4. **Live e2e environment reliability**
   - Keep live tests, but treat local Claude/API connection failures as environment noise unless the SDK path itself fails.
   - Confirm the live tests still exercise stream handling, control responses, and shutdown behavior.

## Handover Notes

- The Rust port is already validated and committed.
- The remaining work is mostly parity and ergonomics, not core runtime shape.
- When implementing the next item, update the tests first or alongside the code.
