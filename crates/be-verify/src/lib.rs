//! `be-verify` — ground-truth verification against the real game (PLAN §4).
//!
//! Phase 2 layer. Provides:
//!
//! - [`locate`] — version-aware `/locate` command generation and a `/locate` output
//!   parser.
//! - [`harness`] — a Bedrock Dedicated Server child-process harness (commands over
//!   stdin, responses over stdout). Tested against a bundled fake-BDS process.
//! - [`verify`] — thin orchestration tying the harness + parser together.
//!
//! **Honesty constraint (PLAN §4 / §2):** Bedrock has no RCON and `/locate` output
//! punctuation is not authoritatively documented and varies between versions. The
//! parser must therefore be built from **captured real output**, and its fixtures are
//! only provisional stand-ins until Phase 0 clears that gate. Until then, results from
//! this crate are **unverified against the real game** and must not be presented as
//! accurate.

pub mod harness;
pub mod locate;
pub mod remote;
pub mod verify;

pub use harness::{BdsConfig, BdsHarness, ReadStrategy};
pub use locate::{LocateCommand, LocateKind, LocateResult};
pub use remote::{RemoteBedrock, RemoteBedrockConfig, RemoteRunner, SshRunner};
