//! Transport layer: spawn and communicate with the `claude` CLI subprocess.
//!
//! Mirrors the Python `SubprocessCLITransport`: finds the binary, builds the
//! command, opens stdin/stdout/stderr, reads newline-delimited JSON, and
//! manages graceful/forceful shutdown.

pub mod subprocess;

pub use subprocess::SubprocessCLITransport;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::ClaudeSdkError;

/// Reader half of a transport: owns stdout, yields parsed JSON messages.
#[async_trait]
pub trait TransportReader: Send {
    /// Read the next parsed JSON message, or `Ok(None)` at EOF.
    async fn read_message(&mut self) -> Result<Option<Value>, ClaudeSdkError>;
}

/// Writer half of a transport: owns stdin, writes raw payloads.
#[async_trait]
pub trait TransportWriter: Send {
    /// Write raw data (typically JSON + newline) to stdin.
    async fn write(&mut self, data: &str) -> Result<(), ClaudeSdkError>;

    /// Close stdin (send EOF).
    async fn end_input(&mut self) -> Result<(), ClaudeSdkError>;
}

/// Abstract transport for Claude communication.
///
/// After [`connect`], call [`split`] to obtain independent reader/writer
/// halves that can be used concurrently without contention.
///
/// [`connect`]: Transport::connect
/// [`split`]: Transport::split
#[async_trait]
pub trait Transport: Send {
    /// Start the underlying process / connection.
    async fn connect(&mut self) -> Result<(), ClaudeSdkError>;

    /// Split into reader and writer halves for concurrent I/O.
    #[allow(clippy::type_complexity)]
    fn split(
        self: Box<Self>,
    ) -> Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>), ClaudeSdkError>;

    /// Fully close the transport and reap the child process.
    async fn close(&mut self) -> Result<(), ClaudeSdkError>;

    /// Whether the transport is ready for read/write.
    fn is_ready(&self) -> bool;
}
