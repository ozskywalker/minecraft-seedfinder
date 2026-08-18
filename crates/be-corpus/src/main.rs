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

use be_biome::{builtin_biome_map, BiomeQuery, CubiomesQuery};
use be_corpus::corpus::BlockPos;
use be_corpus::{
    accuracy, scattered_samples, verify::ANCHOR_STRUCTURES, BiomeSample, Corpus, Sample, Version,
};
use be_verify::{LocateResult, RemoteBedrock, RemoteBedrockConfig};

const USAGE: &str = "usage:\n  be-corpus report <corpus.json> [tolerance]\n  be-corpus generate --version <v> --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus generate-biome --seeds <n> --host <host> [--user <user>] [--biomes <a,b,c>] [--no-biome-namespace] [--container <name>] [--out <corpus.json>]\n  be-corpus report-biome <corpus.json>\n  be-corpus generate-scattered --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus verify-seed --seed <n> --host <host> [--user <user>] [--container <name>] [--structures <a,b,c>] [--tolerance <t>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str);

    match sub {
        Some("report") => cmd_report(&args[2..]),
        Some("generate") => cmd_generate(&args[2..]),
        Some("generate-biome") => cmd_generate_biome(&args[2..]),
        Some("report-biome") => cmd_report_biome(&args[2..]),
        Some("generate-scattered") => cmd_generate_scattered(&args[2..]),
        Some("verify-seed") => cmd_verify_seed(&args[2..]),
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
    let tolerance: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(16);

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
    let user = parse_kv(args, "--user")
        .map(String::as_str)
        .unwrap_or("luser");
    let container = parse_kv(args, "--container");
    // Bedrock >= 1.21.100 requires the minecraft: biome namespace; older versions
    // (e.g. the 1.21.40 validation container) must send bare ids. Default to the
    // namespaced form (matches the live 1.26.43 server); pass --no-biome-namespace
    // for pre-1.21.100 servers.
    let biome_namespace = !args.iter().any(|a| a == "--no-biome-namespace");
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
    cfg.biome_namespace_required = biome_namespace;
    cfg.startup_wait = Duration::from_secs(120);
    // Biome locate responses must flush to `docker logs` before we scrape. 1.5s was
    // too short (matched stale lines from the prior world/seed); 4s is reliable.
    cfg.response_wait = Duration::from_millis(4000);
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

/// `generate-scattered`: capture the shared-salt "scattered" set (PLAN §2.5, #3).
///
/// For each seed, one fresh world, then `/locate structure temple` (which resolves the
/// shared slot of desert_pyramid/igloo/jungle_pyramid/swamp_hut). The single observed
/// position is recorded under each of the four ids (they share placement math), and
/// `be-corpus report` scores each. Requires `--host` (never fabricate ground truth).
fn cmd_generate_scattered(args: &[String]) -> ExitCode {
    let seeds = parse_kv(args, "--seeds").and_then(|s| s.parse::<u32>().ok());
    let host = parse_kv(args, "--host");
    let user = parse_kv(args, "--user")
        .map(String::as_str)
        .unwrap_or("luser");
    let container = parse_kv(args, "--container");
    let out = parse_kv(args, "--out");

    let Some(seeds) = seeds else {
        eprintln!("generate-scattered requires --seeds and --host");
        return ExitCode::from(2);
    };
    let Some(host) = host else {
        eprintln!("generate-scattered requires --host");
        return ExitCode::from(2);
    };

    let mut cfg = RemoteBedrockConfig::live(host, user);
    if let Some(c) = container {
        cfg.container = c.clone();
    }
    cfg.startup_wait = Duration::from_secs(120);
    // 4s: the first /locate right after a fresh world boot can be slow to flush to
    // `docker logs` while chunks generate (1.5s was too short → spurious SKIPs).
    cfg.response_wait = Duration::from_millis(4000);
    let bds = RemoteBedrock::new(cfg);
    let version = Version::builtin_1_21_40();

    let mut corpus = Corpus::new();
    for seed in 0..seeds as i64 {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(seed)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            continue;
        }
        // The first /locate right after a world boot can race chunk generation and
        // return no response; let the freshly-restarted world settle first.
        std::thread::sleep(Duration::from_secs(5));
        match bds.locate(be_corpus::TEMPLE_LOCATE_ID) {
            Ok(Some(LocateResult::Found { x, z, .. })) => {
                let observed = BlockPos::new(x, z);
                let samples = scattered_samples(&version, &version.version, seed as u64, observed);
                eprintln!(
                    "  temple: ({x}, {z}) -> {} scattered sample(s)",
                    samples.len()
                );
                for s in samples {
                    corpus.push(s);
                }
            }
            Ok(Some(LocateResult::NotFound)) => {
                eprintln!("  temple: not found (skipped)");
            }
            Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                eprintln!("  temple: no parseable response (skipped)");
            }
            Err(e) => {
                eprintln!("  temple: locate error: {e} (skipped)");
            }
        }
    }

    let path = out.map(String::as_str).unwrap_or("corpus-scattered.json");
    if let Err(e) = corpus.save(path) {
        eprintln!("error writing corpus {path}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {path}: {} samples", corpus.len());
    ExitCode::SUCCESS
}

/// `verify-seed` (Phase C): verify a returned search-result seed against the real
/// server. Recreates a fresh world of `--seed`, `/locate`s each anchor structure, then
/// checks the model's placement for the region the server chose matches the observation
/// (region-backed-out, same as the corpus gate). Reports PASS/FAIL/SKIP per structure
/// plus an overall verdict. Exits nonzero if any structure FAILs.
fn cmd_verify_seed(args: &[String]) -> ExitCode {
    let seed = parse_kv(args, "--seed").and_then(|s| s.parse::<i64>().ok());
    let host = parse_kv(args, "--host");
    let user = parse_kv(args, "--user")
        .map(String::as_str)
        .unwrap_or("luser");
    let container = parse_kv(args, "--container");
    let tolerance: u64 = parse_kv(args, "--tolerance")
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let structures: Vec<String> = parse_kv(args, "--structures")
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| ANCHOR_STRUCTURES.iter().map(|s| s.to_string()).collect());

    let Some(seed) = seed else {
        eprintln!("verify-seed requires --seed and --host");
        return ExitCode::from(2);
    };
    let Some(host) = host else {
        eprintln!("verify-seed requires --host");
        return ExitCode::from(2);
    };

    let mut cfg = RemoteBedrockConfig::live(host, user);
    if let Some(c) = container {
        cfg.container = c.clone();
    }
    cfg.startup_wait = Duration::from_secs(120);
    // 4s: the first /locate right after a fresh world boot can be slow to flush to
    // `docker logs` while chunks generate (1.5s was too short → spurious SKIPs).
    cfg.response_wait = Duration::from_millis(4000);
    let bds = RemoteBedrock::new(cfg);
    let version = Version::builtin_1_21_40();

    eprintln!("recreating world for seed {seed} ...");
    if let Err(e) = bds.recreate_world(Some(seed)) {
        eprintln!("recreate failed: {e}");
        return ExitCode::FAILURE;
    }
    // The first /locate right after a world boot can race chunk generation; settle.
    std::thread::sleep(Duration::from_secs(5));

    let mut any_fail = false;
    let mut any_skip = false;
    for id in &structures {
        match bds.locate(id) {
            Ok(Some(LocateResult::Found { x, z, .. })) => {
                let observed = BlockPos::new(x, z);
                let predicted = be_corpus::predict_for_region(&version, id, seed as u64, observed);
                let verdict = be_corpus::compare(
                    predicted,
                    Some(LocateResult::Found { x, z, y: None }),
                    tolerance,
                );
                match &verdict {
                    be_corpus::Verdict::Pass => {
                        println!("  {id}: PASS (observed ({x}, {z}))");
                    }
                    be_corpus::Verdict::Fail { reason } => {
                        any_fail = true;
                        println!("  {id}: FAIL — {reason}");
                    }
                    be_corpus::Verdict::Skip { reason } => {
                        any_skip = true;
                        println!("  {id}: SKIP — {reason}");
                    }
                }
            }
            Ok(Some(LocateResult::NotFound)) => {
                any_skip = true;
                println!("  {id}: SKIP — server found no {id} near origin");
            }
            Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                any_skip = true;
                println!("  {id}: SKIP — no parseable locate response");
            }
            Err(e) => {
                any_skip = true;
                println!("  {id}: SKIP — locate error: {e}");
            }
        }
    }

    if any_fail {
        eprintln!("verify-seed: FAILED (seed {seed})");
        ExitCode::FAILURE
    } else if any_skip {
        eprintln!("verify-seed: PASS with skip(s) (seed {seed})");
        ExitCode::SUCCESS
    } else {
        eprintln!("verify-seed: PASS (seed {seed})");
        ExitCode::SUCCESS
    }
}
