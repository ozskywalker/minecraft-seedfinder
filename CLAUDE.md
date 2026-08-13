# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

This project builds a tool for finding Minecraft Bedrock Edition world seeds matching a
constraint graph (structure positions, relative distances, biome gates). The overall
design and the live progress ledger are in `PLAN.md`; read it before working on the
search engine. `AGENTS.md` covers remote-server management for ground-truth validation.
`README.md` is the user-facing build/run doc. This file records agent-relevant build/test
commands and the honesty caveats that still apply.

**Phases 0–5 are complete** (structure RNG/placement, verification harness + corpus,
biome FFI, search engine + DSL + CLI, server + web UI). Phase 6 (optimization) has not
started. **The Phase 0 gate has passed**: both structure placement and the biome gate are
validated GREEN against real Bedrock servers (see "Validation status" below) — results
from this code may be presented as accurate for the validated scope, subject to the open
caveats in that section.

## Current implementation status

- **`crates/be-rng`** — Bedrock's structure RNG. Standard **MT19937** (not Java's 48-bit
  LCG), seeded with the low 32 bits of the region seed. Provides:
  - `MersenneTwister` — full 624-word generator (reference).
  - `first_n(seed, n)` — the **partial-init / streaming twist**: computes exactly the
    first `n` tempered outputs without materializing the 624-word array (working set
    `2(n+1)` words). Valid for `n <= 227`. This is the optimization the property test
    guards.
  - `next_int(bound, raw)` / `next_int(bound)` — `mNextInt`: mask for power-of-two
    bounds, plain **biased** modulo otherwise.
- **`crates/be-struct`** — structure placement math:
  - `region_seed(world_seed, reg_x, reg_z, salt)` — the region-seed formula, masked to
    32 bits.
  - `floor_div(a, b)` — floor division (the game floors; Rust `/` truncates);
    `region_of_block(block, spacing)` — the inverse (which region a block is in).
  - `structure_block_pos(...)` and `structure_block_pos_streaming(...)` — linear (2
    draws) / triangular (4 draws, x then z) placement → block pos `((reg*spacing +
    offset) << 4) + 8`.
  - **Version tables as data** (`versions/1.21.40.json`), loaded via serde
    (`Version::builtin_1_21_40()` embeds it). Only 1.21.x is populated (v1 scope), but
    empirically confirmed accurate across 1.21.x–1.26.43 (see below).
- **`crates/be-verify`** — ground-truth capture (PLAN §4):
  - `locate` — version-aware `/locate` command generation (`minecraft:` biome
    namespace gate at 1.21.100) and a **fixture-driven `/locate` parser**, now built from
    real captured BDS output.
  - `harness` — local BDS child-process harness (stdin/stdout framing, reader thread,
    sentinel or quiet-timeout response strategies), tested against a bundled **fake-BDS**
    process. Kept as a testable abstraction, but **not the production transport**.
  - `remote` — the production transport: Docker-over-SSH driver (`ssh` → `sudo docker
    exec` → `docker logs` scrape) against the project's real Bedrock/Java containers (see
    `AGENTS.md`). Gated behind an explicit `--host` flag in `be-corpus generate`.
  - `verify::locate_structure` — thin orchestration.
- **`crates/be-corpus`** — ground truth + accuracy:
  - Corpus fixture format (JSON): per `(version, seed, structure)` the **observed**
    position, captured from real servers and checked in under `fixtures/`.
  - `compute_accuracy` — per-(version, structure) report: backs out the region
    `/locate` reported (`region_of_block`) and recomputes the prediction with
    `be-struct`, reporting exact/within-tolerance rates, mean/max distance, and the
    mean signed x/z offset. `overall_rate` is the CI gate number — currently **100%**.
  - CLI: `be-corpus report <corpus.json> [tolerance]` (works offline) and
    `be-corpus generate ...` (needs a real BDS via `--host`; see `AGENTS.md`).
- **`crates/cubiomes-sys`** — vendored [cubiomes](https://github.com/Cubitect/cubiomes)
  (MIT, under `cubiomes/`), compiled with `cc`/MSVC (auto-discovered via vswhere — no
  C toolchain on PATH required). Builds only the minimal `getBiomeAt` set
  (`noise, biomes, layers, biomenoise, generator, util`) + a `bridge.c` so Rust never
  needs cubiomes' union-heavy `Generator` layout. Hand-written FFI: `Generator` opaque
  handle, `setup`/`apply_seed`/`biome_at`, `mc_latest`.
- **`crates/be-biome`** — safe biome API:
  - `map` — Bedrock & Java numeric biome ID maps as data (`versions/biomes.json`)
    plus alias resolution for legacy gate names.
  - `query` — `BiomeQuery` trait + `CubiomesQuery` (owns the C `Generator`, RAII free).
  - `gate` — structure biome-gate evaluation (PLAN §2.6).
  - `grid` — 2D grids + `compute_agreement` (predicted vs observed), the validation
    plumbing that closed out the biome gate (below).
- **`crates/be-search`** — the search engine, query model, and CLI:
  - `ir` — the constraint-graph intermediate representation (structures, anchors,
    distance ranges, biome gates).
  - `dsl` — the text DSL (parse + round-trip pretty-print); the same parser backs both
    the CLI and the server.
  - `feasibility` — static pre-check that proves impossible queries instantly (shared
    placement slots, triangle-inequality violations, spacing conflicts) and reports
    *why*, rather than failing after a long search.
  - `planner` — rarest-first join planner with an adaptive mode.
  - `executor` — nested-loop executor with per-seed memoisation, `rayon` parallelism,
    invariant re-checking, and a streaming `search_range_visit` seam (Phase A over the
    low 32 bits, Phase B biome resolution over the high 32 bits).
  - `main` — the `be-search` CLI (`feasibility`, `search` subcommands).
- **`crates/server`** — axum REST + SSE API, and the deliverable entry point:
  - `POST /api/search` — SSE stream of `mode` → `result`s (as found) → `done`, with
    feasibility `note`s surfaced for impossible queries.
  - `GET /api/tile/{seed}/{tx}/{tz}/{lod}` — server-rendered 512-block biome PNG tile
    (quart resolution, LRU-cached, LOD 0–6).
  - `GET /api/catalog` — the structure list (keys, biome gates, shared-slot partners)
    for the UI's route-builder.
  - `assets` — embeds `ui/dist` into the binary at build time (`build.rs`) for the
    single-exe deliverable; falls back to serving `ui/dist` from disk, then a
    placeholder page, so dev builds never break when the UI isn't built.
- **`ui/`** — Vite + React + TypeScript + Tailwind web UI: pan/zoom canvas biome map
  with progressive LOD and precision rebasing, declustered structure markers, a text
  DSL editor driving the SSE stream, a route builder that round-trips to/from DSL, a
  results list (double-click a seed to copy), and URL-encoded shareable state. Not a
  Rust crate — has its own `package.json`/test suite, driven via `build.ps1`/npm.

## Build, test, lint

- **Easiest entry point:** `.\build.ps1` at the repo root — `-Test` runs the full Rust
  workspace tests + UI tests + UI typecheck; default (no flags) builds the production
  single-exe (`dist\seedfinder.exe`); `-Dev` builds the UI + a debug server for local
  iteration. See `AGENTS.md` for details. Requires Rust + Node.js (>=20.19) on the build
  machine; the release exe itself needs neither.
- Install: `cargo build --workspace` (Rust toolchain; deps: serde, serde_json, regex,
  cc, axum, tokio, rayon).
- Full Rust test suite: `cargo test --workspace`
- Single crate: `cargo test -p be-rng` / `-p be-struct` / `-p be-verify` / `-p be-corpus`
  / `-p cubiomes-sys` / `-p be-biome` / `-p be-search` / `-p server`
- Single test: `cargo test -p be-struct placement::tests::streaming_equals_full_generator`
- Integration (fake BDS): `cargo test -p be-verify --test fake_bds_integration`
- Integration (search planner/CLI): `cargo test -p be-search --test planner_invariant`
  / `--test cli`
- Lint: `cargo clippy --workspace --all-targets`
- Web UI (in `ui/`): `npm install`, `npm test` (vitest — DSL, camera/tile math,
  decluster, URL state, SSE, tile fetcher), `npm run build` (`tsc -b && vite build` ->
  `ui/dist`), `npm run typecheck`, `npm audit` (should report 0 vulnerabilities).
- Running the server locally: `cargo run -p server` (serves `ui/dist` on
  `http://127.0.0.1:8080`; build the UI first or you get the placeholder page).
  `SEEDFINDER_ADDR` / `SEEDFINDER_UI_DIR` override address/UI path;
  `SEEDFINDER_NO_OPEN=1` skips auto-opening the browser.
- Production single-exe: `.\scripts\build-release.ps1` -> `dist/seedfinder.exe` (UI
  embedded via `crates/server/build.rs`, auto-opens the browser on startup).

## Validation infrastructure (real servers)

Two real servers run remotely for ground-truth validation. **See `AGENTS.md` for full
management instructions** (SSH + `sudo docker`, `send-command` for Bedrock, RCON for
Java, seed/world control). Summary:

- **Bedrock** (`mc-bedrock`, BDS 1.26.43, port 25570): the authoritative target for
  validating `be-struct` structure placement and the `be-biome` biome gate. Reach the
  console via `docker exec mc-bedrock send-command "locate structure <id>"`; read the
  seed with `send-command "seed"`. Commands need `allow-cheats=true`.
- **Java** (`mc`, 26.2, port 25565): RCON at 25575. Version 26.2 is too new to
  validate cubiomes (≤1.21), so Java is only useful for Bedrock↔Java structure
  differential checks, not biome validation.
- A third, **ephemeral** 1.21.40 container was used once to biome-validate against a
  version-matched server and then removed; see `AGENTS.md` if it needs to be recreated.

## Validation status

**Phase 0 gate: passed.** Both halves are validated against real Bedrock servers, not
just each other:

- **Structure placement: 100%** (6 structures × 5 seeds, exact match) — `be-struct`
  predictions match the real BDS server's `/locate structure` output across the
  1.21.x–1.26.43 range. Reproduce offline: `cargo run -p be-corpus -- report
  fixtures/corpus-1.21.40.json`.
- **Biome gate: GREEN at 100%** for the corpus's surface biomes — cubiomes' `getBiomeAt`
  matches real BDS `/locate biome` output on both a matched-version **1.21.40** server
  and the live **1.26.43** server (`fixtures/biome-corpus-1.21.40.json`,
  `fixtures/biome-corpus-1.21.40.bds.json`). An earlier "18.2%" reading was a y/z
  argument-order bug in `crates/cubiomes-sys/src/bridge.c` (returned `deep_dark` at
  every surface coordinate) — fixed and regression-tested
  (`cubiomes_sys::surface_biome_is_not_deep_dark`,
  `be-corpus::biome_parity_gate_is_green`).

**Remaining honesty caveats** (do not overclaim past these; tracked in `PLAN.md` §8):

- **Shared-salt placement is NOT yet confirmed.** The validated structure corpus
  (village, monument, city, outpost, shipwreck, treasure, portal) does not include the
  shared-salt "scattered" set (desert pyramid / igloo / jungle pyramid / swamp hut).
  Their 100% agreement does not confirm shared-slot behaviour — treat those version-table
  entries as `[UNCONFIRMED]` until a corpus sample covers them.
  - Trial-chamber distribution also has unresolved source conflicts and is flagged
    `[UNCONFIRMED]` in the version tables.
- **Biome parity is empirically observed, not source-proven**, and only for the corpus's
  surface biomes — other biomes/edge cases are unproven. Don't present `be-biome` output
  as universally Bedrock-accurate; it's accurate for what's actually been checked.
- **Corpus version label provenance**: the `1.21.40` corpus label was originally
  captured against the live `1.26.43` server (`UNVERIFIED` provenance at the time), and
  was later re-confirmed with a real 1.21.40 validation container producing
  `biome-corpus-1.21.40.bds.json` (100%) — treat the structure/biome agreement as sound
  across that range, but note the label history if a discrepancy ever surfaces.
- Ground-truth verification of individual *search results* (spawning a real server per
  finalist seed, PLAN §7 "Phase C") is wired but not run in CI — it needs a live Bedrock
  server, so search-result seeds are unverified beyond the structural/biome math above
  until that step is actually run.
- The Phase 5 (server/UI) acceptance runs — open a browser, confirm mode selection and
  VERIFIED badges — are manual and have not been exercised as part of CI.

## Architecture

Seed searching is implemented per `PLAN.md`. Key model to remember:

- **Bedrock seeds are 64-bit, but structure placement uses only the low 32 bits**; the
  high 32 affect biomes/terrain. This splits the search into a structural sweep (Phase A,
  low 32 bits, ns/check, exhaustive over 2³²) and a biome-resolution pass (Phase B, high
  32 bits, ~1000× more expensive, satisficing/sampled — not exhaustive).
- **Completeness honesty**: full 2⁶⁴ enumeration is impossible. The engine is complete
  over the structural subspace for 1–2 origin-anchored variables (exhaustive low-32
  sweep) but gives **no completeness guarantee** once 3+ relational variables or a biome
  gate are involved (satisficing). The CLI/UI/server must always surface which mode ran
  and never imply completeness the engine can't deliver — preserve this when touching
  `be-search::executor` or the server's `/api/search` response shape.
- **Structure parameters are data** (`versions/*.json`), not code, so a new Bedrock
  release is a new JSON file, not a code change.
- The text DSL (`be-search::dsl`) is the single source of truth parsed by both the CLI
  and the server/UI's route builder — keep them sharing that parser rather than growing
  a second implementation.
- **Not yet implemented**: Phase 6 optimization work (the ledger in `PLAN.md` §"Current
  status" is authoritative on what that entails), and running the Phase C real-server
  verification of individual search-result seeds in CI.
