//! Search orchestration for the server (§3.1, PLAN Phase 4 item 6).
//!
//! Runs the structural Phase A + biome Phase B in a background task and streams
//! results as they are found over a `tokio` channel. The SSE handler turns each
//! channel message into a Server-Sent Event, so the UI never blocks on a completed
//! sweep — it receives results the moment the engine emits them. The active mode
//! (exhaustive vs satisficing) is reported first so the UI can be honest about
//! completeness (§3.1).

use std::sync::Arc;

use be_search::{check, parse, plan, BiomeEngine, Engine, Feasibility, Mode, Plan, Query, Version};
use tokio::sync::mpsc;

/// A message streamed to the SSE client.
#[derive(Debug, Clone)]
pub enum SearchEvent {
    /// The mode is running (sent first). `complete` is true for exhaustive.
    Mode { mode: String, complete: bool },
    /// A structural candidate's full seed + positions.
    Result {
        seed: u64,
        positions: Vec<(String, i64, i64)>,
    },
    /// Search finished.
    Done { count: usize },
    /// A non-fatal note (e.g. query infeasible with reasons).
    Note(String),
}

/// A fully-parsed, ready-to-run search job.
pub struct SearchJob {
    pub query: Query,
    pub version: Version,
    pub plan: Plan,
    pub low_start: u32,
    pub low_end: u32,
    pub high_start: u32,
    pub high_end: u32,
    pub max_per_candidate: usize,
    pub include_biomes: bool,
}

impl SearchJob {
    /// Parse a DSL query + bounds into a job, running the feasibility pre-check.
    /// Returns:
    /// - `Err(parse_error)` if the DSL is invalid;
    /// - `Ok(Ok(job))` if feasible and ready to run;
    /// - `Ok(Err(reasons))` if infeasible, with the human-readable reasons (the caller
    ///   should surface these so the user knows *why* the query can't run).
    pub fn from_dsl(
        dsl: &str,
        low_start: u32,
        low_end: u32,
        high_start: u32,
        high_end: u32,
        max_per_candidate: usize,
        include_biomes: bool,
    ) -> Result<Result<Self, Vec<String>>, String> {
        let query = parse(dsl).map_err(|e| e.to_string())?;
        // Feasibility gate first.
        if let Feasibility::Infeasible(reasons) = check(&query) {
            return Ok(Err(reasons));
        }
        let version = Version::builtin_1_21_40();
        let plan = plan(&query);
        Ok(Ok(SearchJob {
            query,
            version,
            plan,
            low_start,
            low_end,
            high_start,
            high_end,
            max_per_candidate,
            include_biomes,
        }))
    }

    /// The mode this job will run in.
    pub fn mode(&self) -> Mode {
        self.plan.mode
    }
}

/// Run the search, streaming events into `tx`. This is the blocking work executed on a
/// `spawn_blocking` thread so the async runtime is never starved.
pub fn run_search(job: Arc<SearchJob>, tx: mpsc::Sender<SearchEvent>) {
    let query = &job.query;
    let engine = Engine {
        query,
        version: &job.version,
        plan: &job.plan,
    };

    // Emit the mode first so the UI can display honesty before any results.
    let mode = job.mode();
    let complete = mode == Mode::Exhaustive;
    let _ = tx.blocking_send(SearchEvent::Mode {
        mode: mode.as_str().to_string(),
        complete,
    });

    let map = be_biome::builtin_biome_map();
    let mc = cubiomes_sys::mc_latest();
    let mut biome_engine = BiomeEngine::new(query, &map, mc);

    let mut count = 0usize;
    // Stream structural candidates as found (Phase A), then resolve biomes per
    // candidate (Phase B). Use a per-candidate resolve so results stream continuously.
    engine.search_range_visit(job.low_start, job.low_end, |cand| {
        if !job.include_biomes {
            let positions = bind_positions(query, &cand.positions);
            let _ = tx.blocking_send(SearchEvent::Result {
                seed: cand.seed,
                positions,
            });
            count += 1;
            return;
        }
        // Phase B: sweep high32 for this candidate and emit each matching full seed.
        let low = cand.seed & 0xFFFF_FFFF;
        let mut emitted = 0usize;
        for high in job.high_start..job.high_end {
            let full = ((high as u64) << 32) | low;
            if biome_engine.passes(full, &cand.positions) {
                let positions = bind_positions(query, &cand.positions);
                let _ = tx.blocking_send(SearchEvent::Result {
                    seed: full,
                    positions,
                });
                count += 1;
                emitted += 1;
                if job.max_per_candidate != 0 && emitted >= job.max_per_candidate {
                    break;
                }
            }
        }
    });

    let _ = tx.blocking_send(SearchEvent::Done { count });
}

/// Convert a candidate's raw positions into (var_name, x, z) triples for the client.
fn bind_positions(query: &Query, positions: &[be_search::Pos]) -> Vec<(String, i64, i64)> {
    query
        .vars
        .iter()
        .zip(positions.iter())
        .map(|(v, p)| (v.name.clone(), p.x, p.z))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_dsl_builds_job_and_reports_mode() {
        let job = SearchJob::from_dsl("village v1 @origin <= 800", 0, 10, 0, 5, 0, false)
            .expect("valid dsl")
            .expect("feasible");
        assert_eq!(job.mode(), Mode::Exhaustive);
    }

    #[test]
    fn infeasible_dsl_yields_reasons() {
        let job = SearchJob::from_dsl(
            "desert_pyramid a @origin <= 2000\njungle_pyramid b @a <= 400",
            0,
            10,
            0,
            5,
            0,
            false,
        )
        .expect("valid dsl");
        match job {
            Ok(_) => panic!("infeasible query must be rejected up front"),
            Err(reasons) => assert!(
                !reasons.is_empty(),
                "infeasible query must carry human-readable reasons"
            ),
        }
    }

    #[test]
    fn run_search_streams_mode_then_results_then_done() {
        let job = Arc::new(
            SearchJob::from_dsl("village v1 @origin <= 800", 0, 30, 0, 2, 0, false)
                .unwrap()
                .unwrap(),
        );
        let (tx, mut rx) = mpsc::channel(16);
        let job2 = job.clone();
        std::thread::spawn(move || run_search(job2, tx));

        // First event is the mode.
        let first = rx.blocking_recv().expect("mode event");
        assert!(matches!(first, SearchEvent::Mode { ref mode, .. } if mode == "exhaustive"));

        // Consume results until Done.
        let mut results = 0usize;
        loop {
            match rx.blocking_recv() {
                Some(SearchEvent::Result { .. }) => results += 1,
                Some(SearchEvent::Done { count }) => {
                    assert_eq!(count, results);
                    break;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }
}
