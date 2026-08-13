//! Integration tests exercising the harness + parser pipeline against the bundled
//! fake-BDS process. `CARGO_BIN_EXE_fake_bds` is only defined for integration tests.

use std::path::PathBuf;
use std::time::Duration;

use be_verify::harness::{BdsConfig, BdsHarness, ReadStrategy};
use be_verify::locate::{parse_locate_output, LocateCommand, LocateResult};

fn fake_bds_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_bds"))
}

fn script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join(name)
}

fn harness(script: &str) -> BdsHarness {
    let cfg = BdsConfig {
        executable: fake_bds_bin(),
        args: vec!["--script".into(), script_path(script).display().to_string()],
        working_dir: None,
        level_seed: Some(1234),
        read_strategy: ReadStrategy::Sentinel("__BDS_RESPONSE_END__".into()),
        startup_wait: Duration::from_millis(50),
    };
    BdsHarness::spawn(&cfg).expect("spawn fake bds")
}

/// End-to-end: spawn the fake BDS, send a /locate command, and recover the
/// coordinate through the harness → parser pipeline.
#[test]
fn harness_round_trip_locate() {
    let mut h = harness("village.json");
    let lines = h
        .command(&LocateCommand::structure("village").render(false))
        .expect("command");
    assert!(lines.iter().any(|l| l.contains("village is at block")));
    let found: Vec<_> = lines
        .iter()
        .filter_map(|l| match parse_locate_output(l) {
            LocateResult::Found { .. } => Some(l.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(
        parse_locate_output(&found[0]),
        LocateResult::Found {
            x: 184,
            z: 296,
            y: None
        }
    );
    h.stop();
}

/// The sentinel strategy must not return the sentinel itself.
#[test]
fn sentinel_is_exclusive() {
    let mut h = harness("village.json");
    let lines = h.command("/locate structure village").expect("command");
    assert!(
        !lines.contains(&"__BDS_RESPONSE_END__".to_string()),
        "sentinel must be stripped"
    );
    h.stop();
}

/// A not-found response flows through cleanly.
#[test]
fn harness_returns_not_found() {
    let mut h = harness("not_found.json");
    let lines = h
        .command("/locate structure ancient_city")
        .expect("command");
    let results: Vec<_> = lines.iter().map(|l| parse_locate_output(l)).collect();
    assert!(results.contains(&LocateResult::NotFound));
    h.stop();
}

/// The verify orchestration wrapper returns a parsed result.
#[test]
fn verify_locates_structure() {
    let mut h = harness("village.json");
    let r = be_verify::verify::locate_structure(&mut h, "village").expect("io");
    assert_eq!(
        r,
        Some(LocateResult::Found {
            x: 184,
            z: 296,
            y: None
        })
    );
    h.stop();
}
