//! `be-corpus` CLI — PLAN §7.3.
//!
//! ```text
//! be-corpus report <corpus.json> [tolerance]
//! be-corpus generate --version <v> --seeds <n> --host <host> [--user <user>]
//!                    [--container <name>] [--out <corpus.json>]
//! ```
//!
//! `report` works offline: it loads a corpus, recomputes predictions with `be-struct`
//! and prints a per-structure accuracy table.
//!
//! `generate` drives the real Dockerized Bedrock server (via SSH, AGENTS.md) to
//! produce ground truth: one fresh world per seed (§4 "one fresh world per seed"), then
//! `/locate` each structure and record the observed position. It refuses to run
//! without `--host`, because fabricating ground truth would silently corrupt the
//! accuracy figure the whole project is built on (Phase 0 gate).

use std::process::ExitCode;
use std::time::Duration;

use be_biome::{BiomeQuery, CubiomesQuery, builtin_biome_map};
use be_corpus::corpus::BlockPos;
use be_corpus::{BiomeSample, Corpus, Sample, Version, accuracy};
use be_verify::{LocateResult, RemoteBedrock, RemoteBedrockConfig};

const USAGE: &str = "usage:\n  be-corpus report <corpus.json> [tolerance]\n  be-corpus generate --version <v> --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus generate-biome --seeds <n> --host <host> [--user <user>] [--biomes <a,b,c>] [--out <corpus.json>]\n  be-corpus report-biome <corpus.json>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str);

    match sub {
        Some("report") => cmd_report(&args[2..]),
        Some("generate") => cmd_generate(&args[2..]),
        Some("generate-biome") => cmd_generate_biome(&args[2..]),
        Some("report-biome") => cmd_report_biome(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn cmd_report(args: &[String]) -> ExitCode {
    let Some(corpus_path) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let tolerance: u64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);

    let corpus = match Corpus::load(corpus_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error loading corpus {corpus_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let version = Version::builtin_1_21_40();
    let report = accuracy::compute_accuracy(&corpus, &version, tolerance);
    print!("{}", report.render());
    match accuracy::overall_rate(&corpus, &version, tolerance) {
        Some(rate) => println!("overall: {:.1}%", rate * 100.0),
        None => println!("overall: n/a (no comparable samples)"),
    }
    ExitCode::SUCCESS
}

fn cmd_generate(args: &[String]) -> ExitCode {
    let mut version: Option<&String> = None;
    let mut seeds: Option<u32> = None;
    let mut host: Option<&String> = None;
    let mut user: Option<&String> = None;
    let mut container: Option<&String> = None;
    let mut out: Option<&String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--version" => {
                i += 1;
                version = args.get(i);
            }
            "--seeds" => {
                i += 1;
                seeds = args.get(i).and_then(|s| s.parse().ok());
            }
            "--host" => {
                i += 1;
                host = args.get(i);
            }
            "--user" => {
                i += 1;
                user = args.get(i);
            }
            "--container" => {
                i += 1;
                container = args.get(i);
            }
            "--out" => {
                i += 1;
                out = args.get(i);
            }
            _ => {}
        }
        i += 1;
    }

    if version.is_none() || seeds.is_none() || host.is_none() {
        eprintln!("generate requires --version, --seeds and --host");
        return ExitCode::from(2);
    }
    let seeds = seeds.unwrap();
    let host = host.unwrap();
    let user = user.map(String::as_str).unwrap_or("luser");

    let mut cfg = RemoteBedrockConfig::live(host, user);
    if let Some(c) = container {
        cfg.container = c.clone();
    }
    // Generation is slow (each world is a full server restart); give generous timeouts.
    cfg.startup_wait = Duration::from_secs(120);
    cfg.response_wait = Duration::from_millis(1500);
    let bds = RemoteBedrock::new(cfg);

    // Which structures to probe. These are the anchor-returning, validated structures
    // (trial_chambers excluded: it returns the bounding-box centre, see golden.rs).
    let structures = [
        "village",
        "ocean_monument",
        "ancient_city",
        "pillager_outpost",
        "shipwreck",
        "buried_treasure",
        "ruined_portal",
    ];

    let mut corpus = Corpus::new();
    for seed in 0..seeds as i64 {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(seed)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            continue;
        }
        for id in structures {
            match bds.locate(id) {
                Ok(Some(LocateResult::Found { x, z, .. })) => {
                    corpus.push(Sample {
                        version: version.unwrap().clone(),
                        seed: seed as u64,
                        structure: id.to_string(),
                        observed: BlockPos::new(x, z),
                    });
                    eprintln!("  {id}: ({x}, {z})");
                }
                Ok(Some(LocateResult::NotFound)) => {
                    eprintln!("  {id}: not found (skipped)");
                }
                Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                    eprintln!("  {id}: no parseable response (skipped)");
                }
                Err(e) => {
                    eprintln!("  {id}: locate error: {e} (skipped)");
                }
            }
        }
    }

    let path = out.map(String::as_str).unwrap_or("corpus.json");
    if let Err(e) = corpus.save(path) {
        eprintln!("error writing corpus {path}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {path}: {} samples", corpus.len());
    ExitCode::SUCCESS
}

fn parse_kv<'a>(args: &'a [String], key: &str) -> Option<&'a String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
}

/// `generate-biome`: one fresh world per seed, then `/locate biome` each target biome,
/// recording the observed position (PLAN §8 parity ground truth).
fn cmd_generate_biome(args: &[String]) -> ExitCode {
    let seeds = parse_kv(args, "--seeds").and_then(|s| s.parse::<u32>().ok());
    let host = parse_kv(args, "--host");
    let user = parse_kv(args, "--user").map(String::as_str).unwrap_or("luser");
    let container = parse_kv(args, "--container");
    let out = parse_kv(args, "--out");

    let Some(seeds) = seeds else {
        eprintln!("generate-biome requires --seeds and --host");
        return ExitCode::from(2);
    };
    let Some(host) = host else {
        eprintln!("generate-biome requires --host");
        return ExitCode::from(2);
    };

    let biomes: Vec<String> = parse_kv(args, "--biomes")
        .map(|s| s.split(',').map(|b| b.trim().to_string()).collect())
        .unwrap_or_else(|| {
            // Common, well-represented Overworld biomes.
            vec![
                "plains".into(),
                "desert".into(),
                "forest".into(),
                "ocean".into(),
            ]
        });

    let mut cfg = RemoteBedrockConfig::live(host, user);
    if let Some(c) = container {
        cfg.container = c.clone();
    }
    cfg.startup_wait = Duration::from_secs(120);
    cfg.response_wait = Duration::from_millis(1500);
    let bds = RemoteBedrock::new(cfg);

    let mut corpus = Corpus::new();
    for seed in 0..seeds as i64 {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(seed)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            continue;
        }
        for name in &biomes {
            match bds.locate_biome(name) {
                Ok(Some(LocateResult::Found { x, z, .. })) => {
                    corpus.push_biome(BiomeSample {
                        version: "1.21.40".into(),
                        seed: seed as u64,
                        biome: name.clone(),
                        observed: BlockPos::new(x, z),
                    });
                    eprintln!("  {name}: ({x}, {z})");
                }
                Ok(Some(LocateResult::NotFound)) => {
                    eprintln!("  {name}: not found (skipped)");
                }
                Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                    eprintln!("  {name}: no parseable response (skipped)");
                }
                Err(e) => {
                    eprintln!("  {name}: locate error: {e} (skipped)");
                }
            }
        }
    }

    let path = out.map(String::as_str).unwrap_or("biome-corpus.json");
    if let Err(e) = corpus.save(path) {
        eprintln!("error writing biome corpus {path}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {path}: {} biome samples", corpus.biome_samples.len());
    ExitCode::SUCCESS
}

/// `report-biome`: compare the real `/locate biome` observations against cubiomes'
/// prediction at the same coordinates (the §8 Bedrock↔Java parity validation).
fn cmd_report_biome(args: &[String]) -> ExitCode {
    let Some(corpus_path) = args.first() else {
        eprintln!("report-biome requires a corpus path");
        return ExitCode::from(2);
    };
    let corpus = match Corpus::load(corpus_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error loading corpus {corpus_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if corpus.biome_samples.is_empty() {
        eprintln!("no biome samples in {corpus_path}");
        return ExitCode::FAILURE;
    }

    let map = builtin_biome_map();
    // cubiomes supports up to 1.21; use the latest constant.
    let mc = cubiomes_sys::mc_latest();

    // Closure: query the Java biome id at a seed's observed coordinate.
    let query_id = |seed: u64, x: i64, z: i64| -> Option<u16> {
        let q = CubiomesQuery::new(mc, seed);
        q.biome_id_at(x as i32, z as i32)
    };
    let resolve = |name: &str| -> Option<u16> { map.bedrock_id_for_name(name) };

    let report = accuracy::compute_biome_agreement(&corpus, query_id, resolve);
    print!("{}", report.render());
    match accuracy::biome_overall_rate(&corpus, query_id, resolve) {
        Some(rate) => println!("overall biome agreement: {:.1}%", rate * 100.0),
        None => println!("overall: n/a (no comparable biome samples)"),
    }
    ExitCode::SUCCESS
}
