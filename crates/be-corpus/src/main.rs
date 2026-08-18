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

const USAGE: &str = "usage:\n  be-corpus report <corpus.json> [tolerance]\n  be-corpus generate --version <v> --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus generate-biome --seeds <n> --host <host> [--user <user>] [--biomes <a,b,c>] [--no-biome-namespace] [--container <name>] [--out <corpus.json>]\n  be-corpus report-biome <corpus.json>\n  be-corpus generate-scattered --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus generate-probe --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus generate-scattered-type --seeds <n> --host <host> [--user <user>] [--container <name>] [--out <corpus.json>]\n  be-corpus report-scattered-type <corpus.json> [--tolerance <t>]\n  be-corpus verify-seed --seed <n> --host <host> [--user <user>] [--container <name>] [--structures <a,b,c>] [--tolerance <t>]\n  be-corpus verify-seeds --host <host> [--seeds <a,b,c>] [--stdin] [--user <user>] [--container <name>] [--structures <a,b,c>] [--tolerance <t>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str);

    match sub {
        Some("report") => cmd_report(&args[2..]),
        Some("generate") => cmd_generate(&args[2..]),
        Some("generate-biome") => cmd_generate_biome(&args[2..]),
        Some("report-biome") => cmd_report_biome(&args[2..]),
        Some("generate-scattered") => cmd_generate_scattered(&args[2..]),
        Some("generate-probe") => cmd_generate_probe(&args[2..]),
        Some("generate-scattered-type") => cmd_generate_scattered_type(&args[2..]),
        Some("report-scattered-type") => cmd_report_scattered_type(&args[2..]),
        Some("verify-seed") => cmd_verify_seed(&args[2..]),
        Some("verify-seeds") => cmd_verify_seeds(&args[2..]),
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
    // 4s (not 1.5s): the first /locate right after a fresh world boot can be slow to
    // flush to `docker logs` while chunks generate — 1.5s produced spurious
    // "no parseable response" SKIPs (AGENTS.md).
    cfg.response_wait = Duration::from_millis(4000);
    let bds = RemoteBedrock::new(cfg);

    // Which structures to probe. These are the anchor-returning, validated structures
    // (trial_chambers excluded: it returns the bounding-box centre, see golden.rs).
    // woodland_mansion is sparse (spacing 80) so it may SKIP on many seeds, but when
    // present the placement check is exact (see ANCHOR_STRUCTURES).
    let structures = [
        "village",
        "ocean_monument",
        "ancient_city",
        "pillager_outpost",
        "shipwreck",
        "buried_treasure",
        "ruined_portal",
        "woodland_mansion",
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

/// `generate-probe`: capture a separate, non-gated probe corpus for low-confidence
/// structures (`ocean_ruin`, `trail_ruins`) whose distribution is `[UNCONFIRMED]`.
///
/// This writes to its **own** fixture and is deliberately separate from the gated
/// `corpus-1.21.40.json` so an inconclusive result cannot trip the 100% regression
/// gate. After capture, run `be-corpus report <probe.json>`; if a probed structure
/// validates cleanly (100%, and `/locate` returns a chunk anchor) it can be promoted
/// into the gated corpus + version table; otherwise the finding is documented.
fn cmd_generate_probe(args: &[String]) -> ExitCode {
    let seeds = parse_kv(args, "--seeds").and_then(|s| s.parse::<u32>().ok());
    let host = parse_kv(args, "--host");
    let out = parse_kv(args, "--out");

    let Some(seeds) = seeds else {
        eprintln!("generate-probe requires --seeds and --host");
        return ExitCode::from(2);
    };
    let Some(host) = host else {
        eprintln!("generate-probe requires --host");
        return ExitCode::from(2);
    };

    let bds = live_bds(host, args);
    // Low-confidence structures with UNCONFIRMED distribution (PLAN §2.8). trial_chambers
    // stays excluded (returns its bounding-box centre, not a chunk anchor).
    let structures = ["ocean_ruin", "trail_ruins"];

    let mut corpus = Corpus::new();
    for seed in 0..seeds as i64 {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(seed)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            continue;
        }
        std::thread::sleep(Duration::from_secs(5));
        for id in structures {
            match bds.locate(id) {
                Ok(Some(LocateResult::Found { x, z, .. })) => {
                    corpus.push(Sample {
                        version: "1.21.40".into(),
                        seed: seed as u64,
                        structure: id.to_string(),
                        observed: BlockPos::new(x, z),
                    });
                    eprintln!("  {id}: ({x}, {z})");
                }
                Ok(Some(LocateResult::NotFound)) => eprintln!("  {id}: not found (skipped)"),
                Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                    eprintln!("  {id}: no parseable response (skipped)");
                }
                Err(e) => eprintln!("  {id}: locate error: {e} (skipped)"),
            }
        }
    }

    let path = out.map(String::as_str).unwrap_or("corpus-probe.json");
    if let Err(e) = corpus.save(path) {
        eprintln!("error writing corpus {path}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {path}: {} samples", corpus.len());
    ExitCode::SUCCESS
}

/// `generate-scattered-type`: capture the data needed to validate scattered-set *type*
/// resolution (PLAN §2.5 / §2.6, #S2).
///
/// For each seed: one fresh world, `/locate structure temple` (the shared scattered
/// slot) and `/locate biome` for each scattered structure's primary gate biome. The
/// temple observation is stored as a `Sample` (structure `"temple"`, which the version
/// table doesn't model, so it's never scored by `report`); the biome observations are
/// stored as `BiomeSample`s. `report-scattered-type` then predicts which type should
/// occupy the slot (via cubiomes at the anchor) and checks it against the server's
/// `/locate biome` layout.
fn cmd_generate_scattered_type(args: &[String]) -> ExitCode {
    let seeds = parse_kv(args, "--seeds").and_then(|s| s.parse::<u32>().ok());
    let host = parse_kv(args, "--host");
    let out = parse_kv(args, "--out");

    let Some(seeds) = seeds else {
        eprintln!("generate-scattered-type requires --seeds and --host");
        return ExitCode::from(2);
    };
    let Some(host) = host else {
        eprintln!("generate-scattered-type requires --host");
        return ExitCode::from(2);
    };

    let bds = {
        let user = parse_kv(args, "--user")
            .map(String::as_str)
            .unwrap_or("luser");
        let mut cfg = be_verify::RemoteBedrockConfig::live(host, user);
        if let Some(c) = parse_kv(args, "--container") {
            cfg.container = c.clone();
        }
        cfg.startup_wait = Duration::from_secs(120);
        // /locate biome is slower to flush than /locate structure; give it longer.
        cfg.response_wait = Duration::from_millis(8000);
        be_verify::RemoteBedrock::new(cfg)
    };
    let version = Version::builtin_1_21_40();
    let map = builtin_biome_map();

    // Primary gate biomes for the four scattered structures, as *valid* `/locate biome`
    // ids. The version-table gate names are legacy aliases (e.g. "icePlains"); Bedrock
    // /locate needs the canonical id ("ice_plains"). Resolve alias -> numeric id ->
    // canonical Bedrock name.
    let mut gate_biomes: Vec<String> = Vec::new();
    for id in be_corpus::SCATTERED_IDS {
        if let Some(gate) = be_corpus::primary_gate_biome(&version, id) {
            if let Some(bid) = map.bedrock_id_for_name(gate) {
                if let Some(name) = map.bedrock_name(bid) {
                    if !gate_biomes.iter().any(|g| g == name) {
                        gate_biomes.push(name.to_string());
                    }
                }
            }
        }
    }
    eprintln!("probing gate biomes: {}", gate_biomes.join(", "));

    let mut corpus = Corpus::new();
    for seed in 0..seeds as i64 {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(seed)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            continue;
        }
        std::thread::sleep(Duration::from_secs(5));
        // Temple slot.
        match bds.locate(be_corpus::TEMPLE_LOCATE_ID) {
            Ok(Some(LocateResult::Found { x, z, .. })) => {
                corpus.push(Sample {
                    version: "1.21.40".into(),
                    seed: seed as u64,
                    structure: "temple".into(),
                    observed: BlockPos::new(x, z),
                });
                eprintln!("  temple: ({x}, {z})");
            }
            Ok(Some(LocateResult::NotFound)) => eprintln!("  temple: not found (skipped)"),
            Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                eprintln!("  temple: no parseable response (skipped)");
            }
            Err(e) => eprintln!("  temple: locate error: {e} (skipped)"),
        }
        // Gate biome layout.
        for name in &gate_biomes {
            match bds.locate_biome(name) {
                Ok(Some(LocateResult::Found { x, z, .. })) => {
                    corpus.push_biome(BiomeSample {
                        version: "1.21.40".into(),
                        seed: seed as u64,
                        biome: name.clone(),
                        observed: BlockPos::new(x, z),
                    });
                }
                Ok(Some(LocateResult::NotFound)) => eprintln!("  {name}: not found (skipped)"),
                Ok(Some(LocateResult::Unparseable(_))) | Ok(None) => {
                    eprintln!("  {name}: no parseable response (skipped)");
                }
                Err(e) => eprintln!("  {name}: locate error: {e} (skipped)"),
            }
        }
    }

    let path = out
        .map(String::as_str)
        .unwrap_or("corpus-scattered-type.json");
    if let Err(e) = corpus.save(path) {
        eprintln!("error writing corpus {path}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "wrote {path}: {} samples, {} biome samples",
        corpus.len(),
        corpus.biome_samples.len()
    );
    ExitCode::SUCCESS
}

/// `report-scattered-type`: offline analysis of a `generate-scattered-type` corpus.
///
/// For each seed it predicts which scattered structure should occupy the temple slot
/// (cubiomes biome at the anchor → the gate that contains it), then checks that the
/// predicted type's primary gate biome is actually present near the slot on the server
/// (via the captured `/locate biome` observation). Prints a per-seed table and a
/// summary. Honest caveat (PLAN §2.6): the game's biome-validity check samples a
/// region, so this is a strong consistency signal, not a proof of the exact placement
/// coordinates the game checks.
fn cmd_report_scattered_type(args: &[String]) -> ExitCode {
    let Some(corpus_path) = args.first() else {
        eprintln!("report-scattered-type requires a corpus path");
        return ExitCode::from(2);
    };
    let tolerance: u64 = parse_kv(args, "--tolerance")
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let corpus = match Corpus::load(corpus_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error loading corpus {corpus_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if corpus.samples.is_empty() {
        eprintln!("no samples in {corpus_path}");
        return ExitCode::FAILURE;
    }

    let version = Version::builtin_1_21_40();
    let map = builtin_biome_map();
    let mc = cubiomes_sys::mc_latest();
    let resolve = |name: &str| -> Option<u16> { map.bedrock_id_for_name(name) };

    let mut seeds: Vec<u64> = corpus
        .samples
        .iter()
        .filter(|s| s.structure == "temple")
        .map(|s| s.seed)
        .collect();
    seeds.sort_unstable();
    seeds.dedup();

    println!("scattered-type report (temple slot biome consistency, within {tolerance} blocks)\n");
    println!(
        "{:<8} {:<10} {:<14} {:<12} {:<12} {:>10}  {:<0}",
        "seed", "type", "biome(id)", "slot", "obs biome", "dist", "verdict"
    );
    let mut confirmed = 0usize;
    let mut total = 0usize;
    let mut no_prediction = 0usize;
    for seed in seeds {
        // Find the temple slot for this seed.
        let slot = corpus
            .samples
            .iter()
            .find(|s| s.seed == seed && s.structure == "temple")
            .map(|s| s.observed);
        let Some(slot) = slot else {
            eprintln!("  seed {seed}: no temple observation");
            continue;
        };
        total += 1;

        let biome_at = |x: i64, z: i64| -> Option<u16> {
            CubiomesQuery::new(mc, seed).biome_id_at(x as i32, z as i32)
        };
        match be_corpus::predict_scattered_type(&version, slot, biome_at, resolve) {
            None => {
                no_prediction += 1;
                println!(
                    "{:<8} {:<10} {:<14} ({}, {:<10}) {:<12} {:>10}  {:<0}",
                    seed,
                    "none",
                    "no-gate",
                    slot.x,
                    slot.z,
                    "-",
                    "-",
                    "no scattered gate biome at slot"
                );
            }
            Some(ty) => {
                let probe = be_corpus::primary_gate_biome(&version, ty).unwrap_or("");
                let obs = corpus
                    .biome_samples
                    .iter()
                    .find(|b| b.seed == seed && b.biome == probe)
                    .map(|b| b.observed);
                match obs {
                    Some(op) => {
                        let dist = (((slot.x - op.x) as f64).powi(2)
                            + ((slot.z - op.z) as f64).powi(2))
                        .sqrt() as u64;
                        let verdict = if dist <= tolerance {
                            "CONFIRMED"
                        } else {
                            "inconsistent"
                        };
                        if dist <= tolerance {
                            confirmed += 1;
                        }
                        println!(
                            "{:<8} {:<10} {:<14} ({}, {:<10}) ({}, {:<10}) {:>10}  {}",
                            seed, ty, probe, slot.x, slot.z, op.x, op.z, dist, verdict
                        );
                    }
                    None => {
                        println!(
                            "{:<8} {:<10} {:<14} ({}, {:<10}) {:<12} {:>10}  {:<0}",
                            seed, ty, probe, slot.x, slot.z, "-", "-", "no biome observation"
                        );
                    }
                }
            }
        }
    }
    println!("\nconfirmed: {confirmed}/{total} (no prediction: {no_prediction})");
    ExitCode::SUCCESS
}

/// `verify-seed` (Phase C): verify a returned search-result seed against the real
/// server. Recreates a fresh world of `--seed`, `/locate`s each anchor structure, then
/// checks the model's placement for the region the server chose matches the observation
/// (region-backed-out, same as the corpus gate). Reports PASS/FAIL/SKIP per structure
/// plus an overall verdict. Exits nonzero if any structure FAILs.
fn cmd_verify_seed(args: &[String]) -> ExitCode {
    let seed = parse_kv(args, "--seed").and_then(|s| parse_seed_token(s));
    let host = parse_kv(args, "--host");
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

    let bds = live_bds(host, args);
    let version = Version::builtin_1_21_40();

    eprintln!("recreating world for seed {seed} ...");
    if let Err(e) = bds.recreate_world(Some(seed as i64)) {
        eprintln!("recreate failed: {e}");
        return ExitCode::FAILURE;
    }
    // The first /locate right after a world boot can race chunk generation; settle.
    std::thread::sleep(Duration::from_secs(5));

    let (any_fail, any_skip) = verify_one_seed(&bds, &version, seed, &structures, tolerance);

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

/// `verify-seeds` (Phase C, multi-seed): like `verify-seed` but for many seeds at once.
///
/// Seeds come from `--seeds a,b,c` and/or from stdin (one per line, when `--stdin` is
/// given or stdin is piped). Each token is parsed as decimal or `0x`-prefixed hex
/// (the search CLI emits plain decimal via `--seeds-only`, which pipes cleanly here).
/// One fresh world per seed, `verify_one_seed` per seed, per-seed summary line, and a
/// single aggregate exit code: nonzero iff any seed had any FAILing structure.
fn cmd_verify_seeds(args: &[String]) -> ExitCode {
    let host = parse_kv(args, "--host");
    let tolerance: u64 = parse_kv(args, "--tolerance")
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let structures: Vec<String> = parse_kv(args, "--structures")
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_else(|| ANCHOR_STRUCTURES.iter().map(|s| s.to_string()).collect());

    let Some(host) = host else {
        eprintln!("verify-seeds requires --host (and --seeds and/or --stdin)");
        return ExitCode::from(2);
    };

    // Collect seeds from --seeds and from stdin.
    let mut seeds: Vec<u64> = Vec::new();
    if let Some(list) = parse_kv(args, "--seeds") {
        for tok in list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            match parse_seed_token(tok) {
                Some(s) => seeds.push(s),
                None => {
                    eprintln!("invalid seed token: {tok:?}");
                    return ExitCode::from(2);
                }
            }
        }
    }
    let stdin_requested = args.iter().any(|a| a == "--stdin");
    // Read stdin when explicitly requested, or when it is piped (not a TTY).
    use std::io::IsTerminal;
    let piped = !std::io::stdin().is_terminal();
    if stdin_requested || piped {
        use std::io::Read;
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_ok() {
            for line in buf.lines() {
                let tok = line.trim();
                if tok.is_empty() {
                    continue;
                }
                match parse_seed_token(tok) {
                    Some(s) => seeds.push(s),
                    None => {
                        eprintln!("invalid seed token from stdin: {tok:?}");
                        return ExitCode::from(2);
                    }
                }
            }
        }
    }
    seeds.sort_unstable();
    seeds.dedup();

    if seeds.is_empty() {
        eprintln!("verify-seeds requires at least one seed (--seeds and/or --stdin)");
        return ExitCode::from(2);
    }

    let bds = live_bds(host, args);
    let version = Version::builtin_1_21_40();

    let mut any_fail = false;
    let mut any_skip = false;
    for seed in &seeds {
        eprintln!("recreating world for seed {seed} ...");
        if let Err(e) = bds.recreate_world(Some(*seed as i64)) {
            eprintln!("  recreate failed for seed {seed}: {e}; skipping");
            any_skip = true;
            continue;
        }
        // Settle so the first /locate after a fresh world boot doesn't race chunk
        // generation (spurious SKIPs).
        std::thread::sleep(Duration::from_secs(5));
        let (f, s) = verify_one_seed(&bds, &version, *seed, &structures, tolerance);
        any_fail |= f;
        any_skip |= s;
    }

    if any_fail {
        eprintln!("verify-seeds: FAILED ({}/{})", any_fail as u8, seeds.len());
        ExitCode::FAILURE
    } else if any_skip {
        eprintln!("verify-seeds: PASS with skip(s) ({} seed(s))", seeds.len());
        ExitCode::SUCCESS
    } else {
        eprintln!("verify-seeds: PASS ({} seed(s))", seeds.len());
        ExitCode::SUCCESS
    }
}

/// Build the live remote driver with the shared timing config used by the Phase C
/// commands (generous startup + 4s response settle; see cmd_generate_scattered).
fn live_bds(host: &str, args: &[String]) -> be_verify::RemoteBedrock {
    let user = parse_kv(args, "--user")
        .map(String::as_str)
        .unwrap_or("luser");
    let mut cfg = be_verify::RemoteBedrockConfig::live(host, user);
    if let Some(c) = parse_kv(args, "--container") {
        cfg.container = c.clone();
    }
    cfg.startup_wait = Duration::from_secs(120);
    cfg.response_wait = Duration::from_millis(4000);
    be_verify::RemoteBedrock::new(cfg)
}

/// Verify one seed's placement for every structure in `structures`, printing a
/// PASS/FAIL/SKIP line per structure. Returns `(any_fail, any_skip)`.
fn verify_one_seed(
    bds: &be_verify::RemoteBedrock,
    version: &Version,
    seed: u64,
    structures: &[String],
    tolerance: u64,
) -> (bool, bool) {
    let mut any_fail = false;
    let mut any_skip = false;
    for id in structures {
        // Bedrock's `/locate` id can differ from the model's structure id (e.g.
        // woodland_mansion -> mansion). Using the model id directly would silently
        // return "No valid structure found".
        let locate = be_corpus::locate_id(id);
        match bds.locate(locate) {
            Ok(Some(LocateResult::Found { x, z, .. })) => {
                let observed = BlockPos::new(x, z);
                let predicted = be_corpus::predict_for_region(version, id, seed, observed);
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
    (any_fail, any_skip)
}

/// Parse a seed token as decimal or `0x`-prefixed hex into a `u64`. The search CLI
/// emits plain decimal via `--seeds-only`; this also accepts `0x`-hex for large seeds.
fn parse_seed_token(tok: &str) -> Option<u64> {
    let t = tok.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}
