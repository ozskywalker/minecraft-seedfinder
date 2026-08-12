//! CLI integration tests: the `be-search` binary's `feasibility` and `search`
//! subcommands behave correctly, offline (PLAN §5 "Integration").
//!
//! Cargo exposes `env!("CARGO_BIN_EXE_be-search")` to integration tests so we can run
//! the built binary directly without a live server.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_be-search"))
}

#[test]
fn feasibility_ok_reports_feasible() {
    let out = bin()
        .args(["feasibility", "village v1 @origin <= 800"])
        .output()
        .expect("run be-search");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("feasible"), "stdout: {stdout}");
}

#[test]
fn feasibility_impossible_reports_reason() {
    // Desert pyramid and jungle pyramid share one slot (§2.5); within 400 blocks is
    // impossible. The CLI must report the *reason*, not just "no results".
    let dsl = "desert_pyramid a @origin <= 2000\njungle_pyramid b @a <= 400";
    let out = bin().args(["feasibility", dsl]).output().expect("run be-search");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("infeasible"), "stdout: {stdout}");
    assert!(stdout.contains("share one placement slot"), "stdout: {stdout}");
}

#[test]
fn search_emits_full_seeds_with_mode() {
    let out = bin()
        .args([
            "search",
            "village v1 @origin <= 800",
            "--low-end",
            "50",
            "--high-end",
            "3",
        ])
        .output()
        .expect("run be-search");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("mode:"), "stdout: {stdout}");
    assert!(stdout.contains("phase A:"), "stdout: {stdout}");
    assert!(stdout.contains("phase B:"), "stdout: {stdout}");
    assert!(stdout.contains("emitted"), "stdout: {stdout}");
    // Seeds are printed as 16-hex-digit lines with a bound position.
    assert!(stdout.contains("v1@("), "stdout: {stdout}");
}

#[test]
fn search_skips_biomes_with_flag() {
    let out = bin()
        .args([
            "search",
            "village v1 @origin <= 800",
            "--low-end",
            "20",
            "--no-biomes",
        ])
        .output()
        .expect("run be-search");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("phase A:"), "stdout: {stdout}");
    assert!(!stdout.contains("phase B:"), "--no-biomes must skip Phase B: {stdout}");
}

#[test]
fn search_refuses_infeasible_query() {
    let dsl = "desert_pyramid a @origin <= 2000\njungle_pyramid b @a <= 400";
    let out = bin().args(["search", dsl]).output().expect("run be-search");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("infeasible"), "stdout: {stdout}");
    assert!(!stdout.contains("emitted"), "must not run a search on an infeasible query");
}

#[test]
fn bad_dsl_is_an_error() {
    let out = bin()
        .args(["search", "village v1 @nowhere <= 800"])
        .output()
        .expect("run be-search");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("DSL error"), "stderr: {stderr}");
}
