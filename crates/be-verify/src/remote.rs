//! Remote Dockerized-BDS driver (PLAN §4 harness, production path).
//!
//! The real Bedrock server in this project runs as a Docker container on a remote
//! host, reached over SSH. It has **no RCON** (PLAN §4): commands are sent through the
//! image's `send-command` helper and responses are scraped from `docker logs`.
//!
//! This is a different transport from the local child-process [`crate::harness`]:
//! here each operation shells out to `ssh ... sudo docker exec ...`. The command
//! sequence for a fresh world per seed (the §4 "one fresh world per seed" model) is:
//!
//! 1. `sed` `level-seed=<seed>` into `/data/server.properties`
//! 2. `rm -rf` the world under `/data/worlds`
//! 3. restart the container (world regenerates with that seed)
//! 4. wait for startup, then `send-command 'locate structure <id>'`
//! 5. read the response back from `docker logs`
//!
//! [`RemoteRunner`] abstracts the SSH invocation so tests can substitute a fake runner
//! and the driver logic is tested without a live server. The real runner requires SSH
//! access to the host (per AGENTS.md).
//!
//! ⚠️ Honesty: this driver talks to the *real* server, so it is gated behind an
//! explicit "live" flag in `be-corpus generate`. It is never invoked by unit tests.

use std::process::Command;
use std::time::{Duration, Instant};

use crate::locate::{LocateResult, parse_locate_output};

/// How to execute a remote shell command (one line).
pub trait RemoteRunner {
    /// Run a single remote command and return its stdout, or an error on nonzero exit.
    fn run(&self, remote_cmd: &str) -> std::io::Result<String>;
}

/// The production runner: `ssh -l <user> <host> <remote_cmd>`.
#[derive(Debug, Clone)]
pub struct SshRunner {
    pub host: String,
    pub user: String,
    /// Extra `ssh` options (e.g. `-o ConnectTimeout=15`). Applied before the host.
    pub ssh_opts: Vec<String>,
}

impl SshRunner {
    pub fn new(host: &str, user: &str) -> Self {
        SshRunner {
            host: host.to_string(),
            user: user.to_string(),
            ssh_opts: vec!["-o".into(), "ConnectTimeout=15".into()],
        }
    }
}

impl RemoteRunner for SshRunner {
    fn run(&self, remote_cmd: &str) -> std::io::Result<String> {
        let mut cmd = Command::new("ssh");
        cmd.args(&self.ssh_opts).arg("-l").arg(&self.user).arg(&self.host).arg(remote_cmd);
        let out = cmd.output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(std::io::Error::other(format!(
                "ssh failed ({:?}): {}",
                out.status,
                err.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

/// Configuration for driving a remote Dockerized Bedrock server.
pub struct RemoteBedrockConfig {
    pub runner: RemoteRunnerBox,
    /// Docker container name on the remote host.
    pub container: String,
    /// World directory inside the container (under `/data/worlds`).
    pub world_dir: String,
    /// How long to wait after restart before the server accepts commands.
    pub startup_wait: Duration,
    /// How long to wait after `send-command` before scraping `docker logs`.
    pub response_wait: Duration,
    /// Whether `/locate biome` ids must carry the `minecraft:` namespace.
    ///
    /// Bedrock requires the namespace since **1.21.100** (PLAN §4). Servers at or
    /// above that version set this to `true`; older versions (e.g. the 1.21.40
    /// validation container) must set it to `false` or every biome locate fails with
    /// a syntax error.
    pub biome_namespace_required: bool,
}

/// Boxed [`RemoteRunner`] so the config is object-safe and clonable.
pub type RemoteRunnerBox = Box<dyn RemoteRunner + Send + Sync>;

impl RemoteBedrockConfig {
    /// Config for the project's live container (`mc-bedrock` on the AGENTS.md host).
    ///
    /// The live server runs BDS 1.26.43 (>= 1.21.100), so the biome namespace is
    /// required by default.
    pub fn live(host: &str, user: &str) -> Self {
        RemoteBedrockConfig {
            runner: Box::new(SshRunner::new(host, user)),
            container: "mc-bedrock".to_string(),
            world_dir: "Bedrock level".to_string(),
            startup_wait: Duration::from_secs(60),
            response_wait: Duration::from_millis(1200),
            biome_namespace_required: true,
        }
    }
}

/// Driver over a remote Dockerized Bedrock server.
pub struct RemoteBedrock {
    cfg: RemoteBedrockConfig,
}

impl RemoteBedrock {
    pub fn new(cfg: RemoteBedrockConfig) -> Self {
        RemoteBedrock { cfg }
    }

    fn docker_exec(&self, inner: &str) -> std::io::Result<String> {
        let c = &self.cfg.container;
        // Use bash -lc so quotes inside the docker command survive.
        let remote = format!(
            "sudo docker exec {c} bash -lc {inner:?}"
        );
        self.cfg.runner.run(&remote)
    }

    /// Current UTC time on the remote host as an RFC3339 string, for `docker logs
    /// --since`. We use the *host* clock (same machine as the containers), so this is
    /// comparable to the container log timestamps.
    fn remote_now_rfc3339(&self) -> std::io::Result<String> {
        let remote = "date -u +%Y-%m-%dT%H:%M:%SZ";
        let out = self.cfg.runner.run(remote)?;
        Ok(out.trim().to_string())
    }

    /// Fetch log lines emitted since `since` (an RFC3339 UTC timestamp), newest last.
    ///
    /// Unlike [`docker_logs_tail`], this is **not** affected by lines that predate the
    /// timestamp — which is what makes it correct across container restarts, where
    /// `docker logs` accumulates output from prior worlds/seeds. See the stale-response
    /// bug this fixes: "newest matching line in the tail" was returning a previous
    /// world's `/locate` result.
    fn docker_logs_since(&self, since: &str, lines: usize) -> std::io::Result<String> {
        let c = &self.cfg.container;
        let remote = format!("sudo docker logs --since {since} --tail {lines} {c}");
        self.cfg.runner.run(&remote)
    }

    /// Set the seed in server.properties. Pass `None` for a random seed (empty).
    pub fn set_seed(&self, seed: Option<i64>) -> std::io::Result<()> {
        let value = seed
            .map(|s| s.to_string())
            .unwrap_or_default();
        // Escape the value for use inside the sed expression.
        let value_escaped = value.replace('&', "\\&").replace('/', "\\/");
        let inner = format!(
            "sed -i \"s/^level-seed=.*/level-seed={value_escaped}/\" /data/server.properties"
        );
        self.docker_exec(&inner)?;
        Ok(())
    }

    /// Read the current level-seed value from server.properties.
    pub fn read_seed_setting(&self) -> std::io::Result<Option<i64>> {
        let inner = "grep '^level-seed=' /data/server.properties";
        let out = self.docker_exec(inner)?;
        let line = out.lines().next().unwrap_or_default();
        let value = line.trim().trim_start_matches("level-seed=").trim();
        if value.is_empty() {
            Ok(None)
        } else {
            value.parse::<i64>().map(Some).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })
        }
    }

    /// Delete the existing world so the next boot regenerates it fresh.
    pub fn delete_world(&self) -> std::io::Result<()> {
        let inner = format!("rm -rf \"/data/worlds/{}\"", self.cfg.world_dir);
        self.docker_exec(&inner)?;
        Ok(())
    }

    /// Restart the container.
    pub fn restart(&self) -> std::io::Result<()> {
        let c = &self.cfg.container;
        let remote = format!("sudo docker restart {c}");
        self.cfg.runner.run(&remote)?;
        Ok(())
    }

    /// Recreate a fresh world with the given seed: set seed, delete world, restart,
    /// then wait for startup. This is the §4 "one fresh world per seed" flow.
    pub fn recreate_world(&self, seed: Option<i64>) -> std::io::Result<()> {
        self.set_seed(seed)?;
        self.delete_world()?;
        self.restart()?;
        self.wait_until_ready()
    }

    /// Poll until the server responds to a command (or timeout).
    ///
    /// Uses a fresh `--since` log window (not the persistent tail), because the
    /// accumulated container log already contains "nearest" lines from prior
    /// worlds/seeds — reading the tail would falsely report "ready" before the
    /// freshly-restarted server has actually come up.
    pub fn wait_until_ready(&self) -> std::io::Result<()> {
        let since = self.remote_now_rfc3339()?;
        let deadline = Instant::now() + self.cfg.startup_wait;
        loop {
            // Probe that the container is up and send-command runs, then check the log
            // responds to a locate within the fresh window.
            if self.docker_exec("true").is_ok() {
                let _ = self.send_locate_raw("village");
                let logs = self.docker_logs_since(&since, 30).unwrap_or_default();
                if logs.contains("nearest") || logs.contains("No valid structure") {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "BDS did not become ready within the startup window",
                ));
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    /// Send a `/locate` command via `send-command` (no response parsing).
    fn send_locate_raw(&self, id: &str) -> std::io::Result<()> {
        let inner = format!("send-command \"locate structure {id}\"");
        self.docker_exec(&inner)?;
        Ok(())
    }

    /// Send a `/locate biome` command via `send-command`.
    ///
    /// The namespace is applied per [`RemoteBedrockConfig::biome_namespace_required`]
    /// (required since Bedrock 1.21.100, PLAN §4), so this driver works against both
    /// pre- and post-1.21.100 servers. `send-command` passes the whole argument to the
    /// server as one token, so the leading `/` must be stripped (the server rejects a
    /// bare `/`).
    fn send_locate_biome_raw(&self, name: &str) -> std::io::Result<()> {
        let cmd = crate::locate::LocateCommand::biome(name)
            .render(self.cfg.biome_namespace_required)
            .trim_start_matches('/')
            .to_string();
        let inner = format!("send-command \"{cmd}\"");
        self.docker_exec(&inner)?;
        Ok(())
    }

    /// Locate a biome's nearest position from origin, scraping the response from logs.
    ///
    /// Bedrock returns a real y for biomes (`at block x, y, z`), so the result carries
    /// `y: Some(...)`. Returns `None` if no response line was found.
    ///
    /// The response is read from `docker logs` only for lines emitted **after** the
    /// command was sent (via a host timestamp), so a stale response from a previous
    /// world/seed in the accumulated container log is never matched. Polls the
    /// `--since` window until a response appears (or [`RemoteBedrockConfig::response_wait`]
    /// elapses), because the first locate after a world boot can be slow to flush while
    /// chunks generate — a single fixed sleep misses it.
    pub fn locate_biome(&self, name: &str) -> std::io::Result<Option<LocateResult>> {
        // Match the biome name as it appears in the response line. On >= 1.21.100 the
        // server echoes the namespaced id (`minecraft:plains`); on older versions it
        // echoes the bare id (`plains`). Use whichever form we sent.
        let marker = if self.cfg.biome_namespace_required {
            format!("minecraft:{name}")
        } else {
            name.to_string()
        };
        let not_found_marker = "Cannot locate the requested biome";
        self.send_locate_biome_raw(name)?;
        self.poll_locate(&marker, not_found_marker)
    }

    /// Locate a structure: send the command, poll the `--since` log window for the
    /// response.
    ///
    /// The locate output always contains the structure's namespaced id (e.g.
    /// `minecraft:village`), so we scan the log for lines emitted **after** the command
    /// (via a host timestamp) that reference that id. Returns `Some(Found)` on a
    /// coordinate response, `Some(...)` for not-found is represented by scanning for
    /// the failure marker too. Returns `None` only if no response line was found.
    pub fn locate(&self, id: &str) -> std::io::Result<Option<LocateResult>> {
        let marker = format!("minecraft:{id}");
        self.send_locate_raw(id)?;
        self.poll_locate(&marker, "No valid structure found")
    }

    /// Send a `/locate` and poll the `--since` window until a response line mentioning
    /// `marker` appears, or until `response_wait` elapses.
    ///
    /// `not_found_marker` is a substring signalling a "not found" result (which also
    /// terminates polling). Returns `Ok(None)` only if neither marker appeared within
    /// the window (no response yet / not flushed).
    fn poll_locate(&self, marker: &str, not_found_marker: &str) -> std::io::Result<Option<LocateResult>> {
        let since = self.remote_now_rfc3339()?;
        let deadline = Instant::now() + self.cfg.response_wait;
        loop {
            let last_after = self.docker_logs_since(&since, 200)?;
            let mut found_lines: Vec<&str> = last_after
                .lines()
                .filter(|l| l.contains(marker))
                .collect();

            if let Some(line) = found_lines.pop() {
                if let LocateResult::Found { x, z, y } = parse_locate_output(line) {
                    return Ok(Some(LocateResult::Found { x, z, y }));
                }
                // A marker line that didn't parse is not a coordinate response; keep
                // polling unless it is the failure marker, handled below.
            }
            if last_after.contains(not_found_marker) {
                return Ok(Some(LocateResult::NotFound));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake runner that records commands and returns canned responses based on
    /// substrings, so the driver logic is tested without a live server.
    struct FakeRunner {
        /// Commands issued, in order (shared so tests can inspect after boxing).
        pub commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        /// Response to return for `docker logs --tail` calls.
        pub logs_tail: String,
        /// Response to return for `send-command` calls (unused for parsing).
        pub send_ok: String,
    }

    impl FakeRunner {
        fn new(logs_tail: &str) -> (Self, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
            let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let r = FakeRunner {
                commands: commands.clone(),
                logs_tail: logs_tail.to_string(),
                send_ok: String::new(),
            };
            (r, commands)
        }
    }

    impl RemoteRunner for FakeRunner {
        fn run(&self, remote_cmd: &str) -> std::io::Result<String> {
            self.commands.lock().unwrap().push(remote_cmd.to_string());
            if remote_cmd.contains("docker logs") {
                Ok(self.logs_tail.clone())
            } else {
                Ok(self.send_ok.clone())
            }
        }
    }

    fn cfg(runner: FakeRunner) -> RemoteBedrockConfig {
        RemoteBedrockConfig {
            runner: Box::new(runner),
            container: "mc-bedrock".into(),
            world_dir: "Bedrock level".into(),
            startup_wait: Duration::from_millis(1),
            response_wait: Duration::from_millis(1),
            biome_namespace_required: true,
        }
    }

    /// A fake runner that returns the given log tail and records issued commands.
    fn cfg_with_logs(logs_tail: &str) -> (RemoteBedrock, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let (runner, commands) = FakeRunner::new(logs_tail);
        (RemoteBedrock::new(cfg(runner)), commands)
    }

    /// Pre-1.21.100 servers must send the bare biome id (no `minecraft:` namespace).
    #[test]
    fn locate_biome_bare_id_on_old_version() {
        let (mut b, commands) = cfg_with_logs(
            "[2026-08-12 13:00:00 INFO] The nearest plains is at block -32, 71, -864 (864 blocks away)",
        );
        b.cfg.biome_namespace_required = false;
        let res = b.locate_biome("plains").unwrap();
        assert_eq!(res, Some(LocateResult::Found { x: -32, z: -864, y: Some(71) }));
        let cmds = commands.lock().unwrap().clone();
        assert!(
            cmds.iter().any(|c| c.contains("locate biome plains") && !c.contains("minecraft:")),
            "expected a bare biome locate command, got {cmds:?}"
        );
    }

    /// >= 1.21.100 servers must send the namespaced biome id.
    #[test]
    fn locate_biome_namespaced_id_on_new_version() {
        let (b, commands) = cfg_with_logs(
            "[2026-08-12 13:00:00 INFO] The nearest minecraft:plains is at block -480, 63, -864 (988 blocks away)",
        );
        // biome_namespace_required defaults to true in cfg().
        let res = b.locate_biome("plains").unwrap();
        assert_eq!(res, Some(LocateResult::Found { x: -480, z: -864, y: Some(63) }));
        let cmds = commands.lock().unwrap().clone();
        assert!(
            cmds.iter().any(|c| c.contains("locate biome minecraft:plains")),
            "expected a namespaced biome locate command, got {cmds:?}"
        );
    }

    #[test]
    fn set_seed_writes_level_seed_line() {
        let (runner, commands) = FakeRunner::new("");
        let b = RemoteBedrock::new(cfg(runner));
        b.set_seed(Some(42)).unwrap();
        let cmds = commands.lock().unwrap().clone();
        // The sed expression targets level-seed and sets it to 42.
        assert!(
            cmds.iter().any(|c| c.contains("level-seed=42")),
            "expected a level-seed=42 sed command, got {cmds:?}"
        );
    }

    #[test]
    fn locate_parses_found_coordinate() {
        let logs = "[2026-08-12 13:00:00 INFO] The nearest minecraft:village is at block 184, (y?), 296 (348 blocks away)";
        let (runner, _) = FakeRunner::new(logs);
        let b = RemoteBedrock::new(cfg(runner));
        let res = b.locate("village").unwrap();
        assert_eq!(res, Some(LocateResult::Found { x: 184, z: 296, y: None }));
    }

    #[test]
    fn locate_returns_not_found_on_failure_marker() {
        let logs = "No valid structure found within a reasonable distance";
        let (runner, _) = FakeRunner::new(logs);
        let b = RemoteBedrock::new(cfg(runner));
        let res = b.locate("mansion").unwrap();
        assert_eq!(res, Some(LocateResult::NotFound));
    }

    #[test]
    fn locate_returns_none_when_no_matching_line() {
        let logs = "[2026-08-12 INFO] some unrelated chatter";
        let (runner, _) = FakeRunner::new(logs);
        let b = RemoteBedrock::new(cfg(runner));
        let res = b.locate("village").unwrap();
        assert_eq!(res, None);
    }

    #[test]
    fn delete_world_targets_the_world_dir() {
        let (runner, commands) = FakeRunner::new("");
        let b = RemoteBedrock::new(cfg(runner));
        b.delete_world().unwrap();
        let cmds = commands.lock().unwrap().clone();
        assert!(
            cmds.iter().any(|c| c.contains("rm -rf") && c.contains("Bedrock level")),
            "expected a delete-world command, got {cmds:?}"
        );
    }
}
