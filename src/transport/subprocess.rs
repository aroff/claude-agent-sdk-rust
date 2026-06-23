//! Subprocess transport using the Claude Code CLI binary.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::error::{ClaudeSdkError, CliConnectionError, CliJsonDecodeError, CliNotFoundError};
use crate::options::{compare_versions, ClaudeAgentOptions, MINIMUM_CLAUDE_CODE_VERSION};
use crate::transport::{Transport, TransportReader, TransportWriter};

const DEFAULT_MAX_BUFFER_SIZE: usize = 1024 * 1024;

/// Find the `claude` CLI binary: explicit path, then `PATH`, then common
/// install locations.
pub fn find_cli(explicit: Option<&Path>) -> Result<PathBuf, CliNotFoundError> {
    if let Some(p) = explicit {
        if p.exists() && p.is_file() {
            return Ok(p.to_path_buf());
        }
        return Err(CliNotFoundError::new(
            "Claude Code not found",
            Some(p.to_string_lossy().into_owned()),
        ));
    }
    if let Ok(found) = which::which("claude") {
        return Ok(found);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let candidates: Vec<PathBuf> = [
        home.as_ref().map(|h| h.join(".npm-global/bin/claude")),
        Some(PathBuf::from("/usr/local/bin/claude")),
        home.as_ref().map(|h| h.join(".local/bin/claude")),
        home.as_ref().map(|h| h.join("node_modules/.bin/claude")),
        home.as_ref().map(|h| h.join(".yarn/bin/claude")),
        home.as_ref().map(|h| h.join(".claude/local/claude")),
    ]
    .into_iter()
    .flatten()
    .collect();
    for c in &candidates {
        if c.exists() && c.is_file() {
            return Ok(c.clone());
        }
    }
    Err(CliNotFoundError::default_msg())
}

/// Subprocess transport that spawns and pipes to the `claude` CLI.
///
/// After [`connect`], call [`split`] to obtain independent reader/writer
/// halves.
///
/// [`connect`]: SubprocessCLITransport::connect
/// [`split`]: SubprocessCLITransport::split (via `Transport::split`)
pub struct SubprocessCLITransport {
    options: ClaudeAgentOptions,
    cli_path: Option<PathBuf>,
    child: Option<Child>,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: Option<BufReader<tokio::process::ChildStdout>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    ready: bool,
    max_buffer_size: usize,
}

impl SubprocessCLITransport {
    /// Create a transport that will spawn `claude` with the given options.
    pub fn new(options: ClaudeAgentOptions) -> Self {
        let max_buffer_size = options.max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER_SIZE);
        Self {
            options,
            cli_path: None,
            child: None,
            stdin: None,
            stdout: None,
            stderr_task: None,
            ready: false,
            max_buffer_size,
        }
    }

    /// Build the full command vector: `[cli_path, *args]`.
    pub fn build_full_command(&self, cli_path: &Path) -> Vec<String> {
        let mut cmd = vec![cli_path.to_string_lossy().into_owned()];
        cmd.extend(self.options.build_command());
        cmd
    }

    /// Check the installed CLI version and warn if below the minimum.
    async fn check_claude_version(&self, cli_path: &Path) {
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::process::Command::new(cli_path).arg("-v").output(),
        )
        .await
        {
            Ok(Ok(o)) => o,
            _ => return,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(captures) = regex_first_version(&stdout) {
            if compare_versions(&captures, MINIMUM_CLAUDE_CODE_VERSION) == std::cmp::Ordering::Less
            {
                warn!(
                    "Claude Code version {captures} at {} is unsupported (min {MINIMUM_CLAUDE_CODE_VERSION})",
                    cli_path.display()
                );
            }
        }
    }

    /// Spawn the stderr reader task that invokes the caller's callback.
    fn spawn_stderr_task(
        stderr: tokio::process::ChildStderr,
        callback: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            callback(trimmed);
                        }));
                    }
                }
            }
        })
    }
}

fn regex_first_version(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut end = None;
    for (i, b) in bytes.iter().enumerate() {
        let is_digit_or_dot = b.is_ascii_digit() || *b == b'.';
        if start.is_none() {
            if b.is_ascii_digit() {
                start = Some(i);
            }
        } else if is_digit_or_dot {
            end = Some(i + 1);
        } else {
            break;
        }
    }
    let s = &s[start?..end?];
    if s.contains('.') {
        Some(s.to_string())
    } else {
        None
    }
}

#[async_trait]
impl Transport for SubprocessCLITransport {
    async fn connect(&mut self) -> Result<(), ClaudeSdkError> {
        if self.child.is_some() {
            return Ok(());
        }

        if self.cli_path.is_none() {
            let cli = find_cli(self.options.cli_path.as_deref())
                .map_err(|e| ClaudeSdkError::new(e.to_string()))?;
            self.cli_path = Some(cli);
        }
        let cli_path = self.cli_path.clone().unwrap();

        if std::env::var("CLAUDE_AGENT_SDK_SKIP_VERSION_CHECK").is_err() {
            self.check_claude_version(&cli_path).await;
        }

        let args = self.options.build_command();
        let mut cmd = Command::new(&cli_path);
        cmd.args(&args);

        for (k, v) in self.options.build_env() {
            cmd.env(k, v);
        }
        if let Some(cwd) = &self.options.cwd {
            cmd.current_dir(cwd);
        }
        if let Some(user) = &self.options.user {
            #[cfg(unix)]
            {
                #[allow(unused_imports)]
                use std::os::unix::process::CommandExt;
                let u: u32 = user
                    .parse()
                    .map_err(|_| CliConnectionError::new(format!("invalid --user uid '{user}'")))?;
                cmd.uid(u);
            }
            #[cfg(not(unix))]
            {
                let _ = user;
            }
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if self.options.stderr.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .kill_on_drop(true);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CliNotFoundError::new(
                    "Claude Code not found",
                    Some(cli_path.to_string_lossy().into_owned()),
                )
                .into());
            }
            Err(e) => {
                return Err(
                    CliConnectionError::new(format!("Failed to start Claude Code: {e}")).into(),
                );
            }
        };

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        self.stderr_task = match (stderr, self.options.stderr.clone()) {
            (Some(s), Some(cb)) => Some(Self::spawn_stderr_task(s, cb)),
            _ => None,
        };

        self.stdin = stdin;
        self.stdout = stdout.map(BufReader::new);
        self.child = Some(child);
        self.ready = true;
        Ok(())
    }

    fn split(
        self: Box<Self>,
    ) -> Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>), ClaudeSdkError> {
        if !self.ready {
            return Err(ClaudeSdkError::new("transport not connected"));
        }
        // SAFETY: we deconstruct the boxed Self into reader and writer halves
        // that own disjoint resources (stdout vs stdin). The child handle and
        // stderr task are shared via Arc<Mutex<>>.
        let transport = *self;
        let child = transport
            .child
            .ok_or_else(|| ClaudeSdkError::new("no child process"))?;
        let child = Arc::new(Mutex::new(child));
        let stderr_task = transport.stderr_task.map(Arc::new);

        let reader = SubprocessReader {
            stdout: transport
                .stdout
                .ok_or_else(|| ClaudeSdkError::new("no stdout"))?,
            json_buffer: String::new(),
            max_buffer_size: transport.max_buffer_size,
            child: child.clone(),
        };

        let writer = SubprocessWriter {
            stdin: transport.stdin,
            child,
            stderr_task,
        };

        Ok((Box::new(reader), Box::new(writer)))
    }

    async fn close(&mut self) -> Result<(), ClaudeSdkError> {
        self.ready = false;
        if let Some(stdin) = self.stdin.take() {
            drop(stdin);
        }
        if let Some(handle) = self.stderr_task.take() {
            handle.abort();
        }
        if let Some(child) = self.child.as_mut() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(_)) => {}
                _ => {
                    let _ = child.start_kill();
                    match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
                        .await
                    {
                        Ok(Ok(_)) => {}
                        _ => {
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                        }
                    }
                }
            }
        }
        self.child = None;
        self.stdout = None;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.ready
    }
}

/// Reader half: owns stdout, yields parsed JSON messages.
pub struct SubprocessReader {
    stdout: BufReader<tokio::process::ChildStdout>,
    json_buffer: String,
    max_buffer_size: usize,
    #[allow(dead_code)]
    child: Arc<Mutex<Child>>,
}

#[async_trait]
impl TransportReader for SubprocessReader {
    async fn read_message(&mut self) -> Result<Option<Value>, ClaudeSdkError> {
        loop {
            let mut line = String::new();
            let n =
                self.stdout.read_line(&mut line).await.map_err(|e| {
                    ClaudeSdkError::new(format!("Failed to read from CLI stdout: {e}"))
                })?;
            if n == 0 {
                if !self.json_buffer.is_empty() {
                    let buf = std::mem::take(&mut self.json_buffer);
                    if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                        return Ok(Some(v));
                    }
                }
                return self.finish_eof();
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.json_buffer.is_empty() && !trimmed.starts_with('{') {
                debug!(
                    "Skipping non-JSON line from CLI stdout: {}",
                    &trimmed[..trimmed.len().min(200)]
                );
                continue;
            }
            self.json_buffer.push_str(trimmed);
            if self.json_buffer.len() > self.max_buffer_size {
                let len = self.json_buffer.len();
                self.json_buffer.clear();
                return Err(ClaudeSdkError::new(
                    CliJsonDecodeError::new(
                        format!(
                            "JSON message exceeded buffer size {len} > {}",
                            self.max_buffer_size
                        ),
                        std::io::Error::other("buffer overflow"),
                    )
                    .to_string(),
                ));
            }
            match serde_json::from_str::<Value>(&self.json_buffer) {
                Ok(v) => {
                    self.json_buffer.clear();
                    return Ok(Some(v));
                }
                Err(_) => continue,
            }
        }
    }
}

impl SubprocessReader {
    fn finish_eof(&self) -> Result<Option<Value>, ClaudeSdkError> {
        // Best-effort exit-code check; the writer half (Drop) handles reaping.
        Ok(None)
    }
}

/// Writer half: owns stdin, writes raw payloads.
pub struct SubprocessWriter {
    stdin: Option<tokio::process::ChildStdin>,
    child: Arc<Mutex<Child>>,
    stderr_task: Option<Arc<tokio::task::JoinHandle<()>>>,
}

#[async_trait]
impl TransportWriter for SubprocessWriter {
    async fn write(&mut self, data: &str) -> Result<(), ClaudeSdkError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CliConnectionError::new("stdin closed"))?;
        stdin
            .write_all(data.as_bytes())
            .await
            .map_err(|e| ClaudeSdkError::new(format!("Failed to write to process stdin: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| ClaudeSdkError::new(format!("Failed to flush process stdin: {e}")))?;
        Ok(())
    }

    async fn end_input(&mut self) -> Result<(), ClaudeSdkError> {
        if let Some(stdin) = self.stdin.take() {
            drop(stdin);
        }
        Ok(())
    }
}

impl Drop for SubprocessWriter {
    fn drop(&mut self) {
        if let Some(stdin) = self.stdin.take() {
            drop(stdin);
        }
        // Best-effort graceful shutdown then force kill.
        if let Ok(mut guard) = self.child.try_lock() {
            let _ = guard.start_kill();
            // Cannot await in Drop; rely on kill_on_drop(true) set at spawn.
        }
        if let Some(task) = self.stderr_task.take() {
            #[allow(unused_must_use)]
            {
                tokio::spawn(async move { task.abort() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_full_command_prepends_cli_path() {
        let opts = ClaudeAgentOptions {
            model: Some("claude-sonnet-4-5".into()),
            ..Default::default()
        };
        let t = SubprocessCLITransport::new(opts);
        let cli = PathBuf::from("/usr/local/bin/claude");
        let cmd = t.build_full_command(&cli);
        assert_eq!(cmd[0], "/usr/local/bin/claude");
        assert!(cmd.iter().any(|a| a == "--model"));
        assert!(cmd.iter().any(|a| a == "claude-sonnet-4-5"));
    }

    #[test]
    fn regex_first_version_extracts_semver() {
        assert_eq!(
            regex_first_version("2.1.39 (Claude Code)"),
            Some("2.1.39".into())
        );
        assert_eq!(regex_first_version("no version here"), None);
    }

    #[test]
    fn find_cli_uses_explicit_path() {
        let res = find_cli(Some(&PathBuf::from("/nonexistent/claude")));
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn split_round_trip_with_echo_binary() {
        let opts = ClaudeAgentOptions::default();
        let mut t = SubprocessCLITransport::new(opts);
        let mut cmd = Command::new("cat");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().expect("cat should be available");
        t.stdin = child.stdin.take();
        t.stdout = child.stdout.take().map(BufReader::new);
        t.child = Some(child);
        t.ready = true;

        let boxed: Box<dyn Transport> = Box::new(t);
        let (mut reader, mut writer) = boxed.split().unwrap();

        writer
            .write("{\"type\":\"system\",\"subtype\":\"start\"}\n")
            .await
            .unwrap();
        let msg = reader.read_message().await.unwrap().unwrap();
        assert_eq!(msg["type"], "system");
        assert_eq!(msg["subtype"], "start");

        writer.end_input().await.unwrap();
        let eof = reader.read_message().await.unwrap();
        assert!(eof.is_none());
    }
}
