//! `be-search` CLI — exercise the Phase 4 search engine offline (no `--host`, no
//! browser). Mirrors the hand-rolled arg style of `be-corpus`.
//!
//! ```text
//! be-search feasibility '<dsl>'
//! be-search search '<dsl>' [--low-start N] [--low-end N] [--high-start N]
//!            [--high-end N] [--max-per-candidate N] [--no-biomes]
//! ```
//!
//! - `feasibility` runs the static pre-check and prints OK or the reasons.
//! - `search` runs Phase A (structural sweep over low 32 bits) then Phase B (biome
//!   resolution over the high 32 bits) and prints each matching full seed with its
//!   bound positions. Pass `--no-biomes` to skip Phase B (structure-only).
//!
//! Both are offline and deterministic. The search is **satisficing** for the high-32
//! sweep; the emitted mode is always printed so the UI/CLI never lies about
//! completeness (§3.1).

use std::process::ExitCode;

use be_biome::builtin_biome_map;
use be_search::{check, parse, plan, BiomeEngine, Engine, Feasibility, Version};

const USAGE: &str = "\
usage:
  be-search feasibility '<dsl>'
  be-search search '<dsl>' [--low-start N] [--low-end N] [--high-start N]
             [--high-end N] [--max-per-candidate N] [--no-biomes]
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str);
    match sub {
        Some("feasibility") => cmd_feasibility(&args[2..]),
        Some("search") => cmd_search(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_kv(args: &[String], key: &str) -> Option<u64> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
}

fn cmd_feasibility(args: &[String]) -> ExitCode {
    let Some(dsl) = args.first() else {
        eprintln!("feasibility requires a DSL query");
        return ExitCode::from(2);
    };
    let query = match parse(dsl) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("DSL error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match check(&query) {
        Feasibility::Ok => {
            println!("feasible");
            ExitCode::SUCCESS
        }
        Feasibility::Infeasible(reasons) => {
            println!("infeasible:");
            for r in &reasons {
                println!("  - {r}");
            }
            ExitCode::SUCCESS
        }
    }
}

fn cmd_search(args: &[String]) -> ExitCode {
    let Some(dsl) = args.first() else {
        eprintln!("search requires a DSL query");
        return ExitCode::from(2);
    };
    let low_start = parse_kv(args, "--low-start").unwrap_or(0) as u32;
    let low_end = parse_kv(args, "--low-end").unwrap_or(1000) as u32;
    let high_start = parse_kv(args, "--high-start").unwrap_or(0) as u32;
    let high_end = parse_kv(args, "--high-end").unwrap_or(100) as u32;
    let max_per = parse_kv(args, "--max-per-candidate").unwrap_or(0) as usize;
    let no_biomes = args.iter().any(|a| a == "--no-biomes");

    let query = match parse(dsl) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("DSL error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Feasibility gate first: refuse to run an impossible query.
    if let Feasibility::Infeasible(reasons) = check(&query) {
        println!("query is infeasible:");
        for r in &reasons {
            println!("  - {r}");
        }
        return ExitCode::SUCCESS;
    }

    let version = Version::builtin_1_21_40();
    let plan = plan(&query);
    let engine = Engine {
        query: &query,
        version: &version,
        plan: &plan,
    };

    println!(
        "mode: {}",
        match plan.mode {
            be_search::Mode::Exhaustive => "exhaustive (structural: complete over low32)",
            be_search::Mode::Satisficing => "satisficing (no completeness guarantee)",
        }
    );
    println!("phase A: structural sweep over low32 in {low_start}..{low_end}");

    let structural = engine.search_range(low_start, low_end);
    println!("phase A: {} structural candidates", structural.len());

    if no_biomes {
        for c in &structural {
            print_candidate(&query, c);
        }
        return ExitCode::SUCCESS;
    }

    let map = builtin_biome_map();
    let mc = cubiomes_sys::mc_latest();
    let mut biome_engine = BiomeEngine::new(&query, &map, mc);
    println!("phase B: biome resolution over high32 in {high_start}..{high_end} (satisficing)");

    let mut emitted = 0usize;
    let filtered = biome_engine.resolve_biomes(&structural, high_start, high_end, max_per);
    for c in &filtered {
        // Invariant re-check before display (structural + biome).
        if engine.verify(c) && biome_engine.verify(c) {
            print_candidate(&query, c);
            emitted += 1;
        }
    }
    println!("emitted {} full-seed result(s)", emitted);
    ExitCode::SUCCESS
}

fn print_candidate(query: &be_search::Query, c: &be_search::Candidate) {
    let mut parts: Vec<String> = Vec::new();
    for (i, var) in query.vars.iter().enumerate() {
        let p = c.positions[i];
        parts.push(format!("{}@({},{})", var.name, p.x, p.z));
    }
    println!("seed {:016x} {}", c.seed, parts.join(" "));
}
