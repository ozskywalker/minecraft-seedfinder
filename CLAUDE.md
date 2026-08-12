# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

This project builds a tool for finding Minecraft Bedrock Edition world seeds matching a
constraint graph (structure positions, relative distances, biome gates). The overall
design is in `PLAN.md`; read it before working on the search engine. This file records
the currently-implemented slice and its build/test commands.

## Current implementation status

Phases 1–2 are underway. Implemented and tested:

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
    (`Version::builtin_1_21_40()` embeds it). Only 1.21.x is populated (v1 scope).
- **`crates/be-verify`** — ground-truth capture (PLAN §4):
  - `locate` — version-aware `/locate` command generation (`minecraft:` biome
    namespace gate at 1.21.100) and a **fixture-driven `/locate` parser**.
  - `harness` — BDS child-process harness (stdin/stdout framing, reader thread,
    sentinel or quiet-timeout response strategies). Tested against a bundled
    **fake-BDS** process (`fake_bds` bin + `tests/scripts/*.json`), so the plumbing is
    proven without a real server.
  - `verify::locate_structure` — thin orchestration.
- **`crates/be-corpus`** — ground truth + accuracy:
  - Corpus fixture format (JSON): per `(version, seed, structure)` the **observed**
    position.
  - `compute_accuracy` — per-(version, structure) report: backs out the region
    `/locate` reported (`region_of_block`) and recomputes the prediction with
    `be-struct`, reporting exact/within-tolerance rates, mean/max distance, and the
    **mean signed x/z offset** (which directly surfaces the [UNCONFIRMED]
    anchor-vs-centre problem). `overall_rate` is the CI gate number.
  - CLI: `be-corpus report <corpus.json> [tolerance]` (works offline) and
    `be-corpus generate ...` (refuses to run without a real BDS).

## Build, test, lint

- Install: `cargo build --workspace` (Rust toolchain; deps: serde, serde_json, regex, cc).
- Full test suite: `cargo test --workspace`
- Single crate: `cargo test -p be-rng` / `-p be-struct` / `-p be-verify` / `-p be-corpus`
  / `-p cubiomes-sys` / `-p be-biome`
- Single test: `cargo test -p be-struct placement::tests::streaming_equals_full_generator`
- Integration (fake BDS): `cargo test -p be-verify --test fake_bds_integration`
- Lint: `cargo clippy --workspace --all-targets`

## Validation infrastructure (real servers)

Two real servers run remotely for Phase 0/2/3 ground-truth validation. **See
`AGENTS.md` for full management instructions** (SSH + `sudo docker`, `send-command`
for Bedrock, RCON for Java, seed/world control). Summary:

- **Bedrock** (`mc-bedrock`, BDS 1.26.43, port 25570): the authoritative target for
  validating `be-struct` structure placement. Reach the console via
  `docker exec mc-bedrock send-command "locate structure <id>"`; read the seed with
  `send-command "seed"`. Commands need `allow-cheats=true`.
- **Java** (`mc`, 26.2, port 25565): RCON at 25575. Version 26.2 is too new to
  validate cubiomes (≤1.21), so Java is only useful for Bedrock↔Java structure
  differential checks, not biome validation.

## Phase 3 note

Phase 3 (biomes) is partly implemented:

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
    plumbing for LevelDB `Data3D`.

## Architecture

Seed searching is implemented per `PLAN.md`. Key model to remember:

- **Bedrock seeds are 64-bit, but structure placement uses only the low 32 bits**; the
  high 32 affect biomes/terrain. This splits the search into a structural sweep (low
  32, ns/check) and a biome-resolution pass (high 32, µs/check).
- **Structure placement is deterministic and cheap** (region-seed formula + small MT
  draw), so structural geometry is exhaustively searchable over 2³².
- **Biome evaluation is ~1000× more expensive** and must run last.
- Structure parameters are **data** (`versions/*.json`), not code, so a new Bedrock
  release is a new JSON file.
- Not yet implemented (future phases): constraint graph + join planner, real-BDS corpus
  generation (`be-corpus generate` is gated on the Phase 0 gate), LevelDB `Data3D`
  decoding/validation, server/UI, feasibility pre-check.

## Phase 0 gate

`PLAN.md` requires verifying the generator's predictions against a real Bedrock
Dedicated Server before building the search engine. Until that gate is cleared, results
from this code are unverified against the real game and must not be presented as
accurate.

**The `/locate` parser fixtures (`be-verify`) are provisional stand-ins, not real
capture.** The plan mandates building the parser from captured BDS output (message
punctuation varies by version). The parser is fixture-driven and version-gated so real
output can be dropped in, but it has not been validated against a live server. The
bundled `fake_bds` scripts exercise the harness plumbing only — they are not ground
truth.

**`cubiomes-sys`/`be-biome` return *Java* biome ids, not Bedrock.** Bedrock↔Java biome
parity is empirically observed but not proven (PLAN §8). The `compute_agreement` layer
in `be-biome` is the mechanism that would validate it against real LevelDB `Data3D`
grids, but no real Bedrock world data is present in this environment, so biome results
must not be presented as Bedrock-accurate.
