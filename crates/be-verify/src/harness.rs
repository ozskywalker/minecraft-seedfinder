//! BDS child-process harness.
//!
//! Bedrock Dedicated Server has **no RCON** (PLAN §4): commands go in over stdin,
//! responses come back on stdout. This harness spawns BDS (or, in tests, a fake BDS)
//! and provides framed request/response over those pipes.
//!
//! Because BDS output is line-oriented and unbounded log chatter interleaves with
//! command responses, a dedicated reader thread continuously drains stdout into a
//! channel. `read_response` then collects lines according to a [`ReadStrategy`]:
//!
//! - [`ReadStrategy::Sentinel`] — read until a sentinel line (used by the fake BDS
//!   and by tests).
//! - [`ReadStrategy::QuietTimeout`] — read lines until the pipe goes quiet for a
//!   duration (intended for real BDS, where response framing must be tuned against
//!   captured output).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

/// How to decide a command response is complete.
#[derive(Debug, Clone)]
pub enum ReadStrategy {
    /// Read until this exact trimmed line is seen (exclusive).
    Sentinel(String),
    /// Read until the stdout pipe is quiet for this long.
    QuietTimeout(Duration),
}

impl Default for ReadStrategy {
    fn default() -> Self {
        ReadStrategy::QuietTimeout(Duration::from_millis(200))
    }
}

/// Configuration for spawning a BDS (or fake BDS) child process.
#[derive(Debug, Clone)]
pub struct BdsConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    /// World seed passed to BDS (e.g. via `LEVEL_SEED` or a command). Stored for
    /// provenance; the harness itself just spawns the process.
    pub level_seed: Option<u64>,
    pub read_strategy: ReadStrategy,
    /// How long to wait after spawn for the server to come up before first command.
    pub startup_wait: Duration,
}

impl BdsConfig {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        BdsConfig {
            executable: executable.into(),
            args: Vec::new(),
            working_dir: None,
            level_seed: None,
            read_strategy: ReadStrategy::default(),
            startup_wait: Duration::from_millis(100),
        }
    }
}

/// A live session with a (fake) BDS child process.
pub struct BdsHarness {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    strategy: ReadStrategy,
}

impl BdsHarness {
    /// Spawn the child and begin draining stdout.
    pub fn spawn(cfg: &BdsConfig) -> std::io::Result<BdsHarness> {
        let mut cmd = Command::new(&cfg.executable);
        cmd.args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(wd) = &cfg.working_dir {
            cmd.current_dir(wd);
        }
        let mut child = cmd.spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no stdin"))?;
        let stdout: ChildStdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no stderr"))?;

        let (tx, rx) = mpsc::channel();
        // Drain stdout into the channel.
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line.trim_end().to_string()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        // Drain stderr so the child never blocks on a full stderr pipe.
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut sink = String::new();
            loop {
                sink.clear();
                match reader.read_line(&mut sink) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });

        if !cfg.startup_wait.is_zero() {
            std::thread::sleep(cfg.startup_wait);
        }

        Ok(BdsHarness {
            child,
            stdin,
            rx,
            strategy: cfg.read_strategy.clone(),
        })
    }

    /// Write a raw command line to BDS stdin.
    pub fn send(&mut self, line: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()
    }

    /// Collect the response to the previous command according to the strategy.
    pub fn read_response(&mut self) -> std::io::Result<Vec<String>> {
        match &self.strategy {
            ReadStrategy::Sentinel(s) => {
                let mut out = Vec::new();
                while let Ok(line) = self.rx.recv() {
                    if line == *s {
                        break;
                    }
                    if !line.is_empty() {
                        out.push(line);
                    }
                }
                Ok(out)
            }
            ReadStrategy::QuietTimeout(d) => {
                let mut out = Vec::new();
                loop {
                    match self.rx.recv_timeout(*d) {
                        Ok(line) => {
                            if !line.is_empty() {
                                out.push(line);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                Ok(out)
            }
        }
    }

    /// Send a command and read its full response.
    pub fn command(&mut self, line: &str) -> std::io::Result<Vec<String>> {
        self.send(line)?;
        self.read_response()
    }

    /// Terminate the child (best-effort).
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for BdsHarness {
    fn drop(&mut self) {
        self.stop();
    }
}
