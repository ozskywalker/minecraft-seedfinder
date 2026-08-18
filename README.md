# Bedrock Seedfinder

A tool for finding **Minecraft Bedrock Edition** world seeds that match a *constraint
graph*: structure positions, relative distances between structures, and biome gates.
For example, "a desert pyramid within 600–1200 blocks of a village, whose anchor biome
is desert, with an ocean monument within 1500 blocks."

The search engine, CLI, and a local web UI are all implemented in this repository. The
full design and rationale live in [`PLAN.md`](PLAN.md); this README covers the current,
working slice and how to build/run it.

> **Status:** Phases 0–5 are implemented and tested. Structure placement and the biome
> gate are **validated against real Bedrock servers** (see
> [Validation](#validation--ground-truth) below). Phase 6 (optimization) has begun with
> seed-lookup speedups (see `PLAN.md`).

---

## How it works

Bedrock world seeds are 64-bit, but **structure placement uses only the low 32 bits**;
the high 32 bits affect biomes and terrain. This splits the search into two passes:

- **Phase A — structural sweep over the low 32 bits.** Each seed's structure geometry is
  deterministic and cheap to compute (a region-seed formula plus a small MT19937 draw),
  so it is exhaustively searchable over 2³².
- **Phase B — biome resolution over the high 32 bits.** For each structural candidate,
  sweep high-32 values, build the full 64-bit seed, and ask [cubiomes] whether each
  structure's anchor is in its acceptable biome set. This is ~1000× more expensive, so it
  runs last and is **satisficing** (samples a range, not all 2³²).

A **feasibility pre-check** statically proves impossible queries *instantly* (e.g.
"desert pyramid within 400 blocks of jungle temple" — they share one placement slot
every 512 blocks) and reports the *reason*, rather than failing after a ten-minute
search.

> ⚠️ **Completeness honesty.** Full 2⁶⁴ enumeration is impossible. The engine either
> runs an **exhaustive** sweep of all 2³² low halves (1–2 origin-anchored variables —
> complete over the structural subspace) or a **satisficing** sweep (3+ relational
> variables — **no completeness guarantee**). The UI and CLI always report which mode is
> running and never imply completeness the engine cannot deliver.

[cubiomes]: https://github.com/Cubitect/cubiomes

## Repository layout

```
├─ crates/
│  ├─ be-rng/        MT19937 + partial-init/streaming twist + mNextInt bias   [pure Rust]
│  ├─ be-struct/     region math, placement, version tables                   [pure Rust]
│  ├─ cubiomes-sys/  vendored cubiomes (MIT), cc-built FFI bindings           [C FFI]
│  ├─ be-biome/      safe biome API over cubiomes + Bedrock ID map
│  ├─ be-search/     constraint model, feasibility, rarest-first planner,
│  │                 engine (Phase A + B), text DSL, CLI
│  ├─ be-verify/     BDS /locate parser + remote Docker-over-SSH driver
│  ├─ be-corpus/     ground-truth fixture format + accuracy reporting
│  └─ server/        axum REST + SSE API, server-side tile renderer, serves the UI
├─ ui/               Vite + React + TypeScript + Tailwind canvas web UI
├─ scripts/          build-release.ps1 — produce the single self-contained .exe
├─ versions/         1.21.x.json — structure params + biome gates (data, not code)
├─ fixtures/         BDS-derived ground-truth corpus (checked in)
├─ PLAN.md           the design document and progress ledger
└─ AGENTS.md         remote-server management & validation infrastructure notes
```

## Prerequisites

- **Rust** (stable) — builds the workspace; a C toolchain is auto-discovered for the
  vendored cubiomes (`cc`/MSVC via vswhere, no C on PATH required).
- **Node.js ≥ 20.19** (or ≥ 22.12) — builds the web UI (`ui/`); required by Vite 8.

## Build, test, lint

```bash
# Rust — everything
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets

# One crate / one test
cargo test -p be-search
cargo test -p server

# Web UI — install, unit-test the pure logic, and build ui/dist
cd ui
npm install
npm test          # vitest run (DSL, camera/tile math, decluster, URL state, SSE, tile fetcher)
npm run build     # tsc -b && vite build -> ui/dist
npm audit         # should report 0 vulnerabilities

# Production single-exe (embeds the UI + auto-opens the browser)
.\scripts\build-release.ps1   # -> dist/seedfinder.exe
```

## Running the server + UI

The server serves the built UI from `ui/dist` and exposes the search/tile/catalog APIs.

```bash
# Build the UI first (so / serves the app, not the placeholder page)
cd ui && npm run build && cd ..

# Run the server (default http://127.0.0.1:8080)
cargo run -p server

# Custom address / UI dir
SEEDFINDER_ADDR=127.0.0.1:9000 SEEDFINDER_UI_DIR=/path/to/ui/dist cargo run -p server
```

Open `http://127.0.0.1:8080` in a browser: pan/zoom the biome map, enter a DSL query or
build a route visually, and watch results stream in over SSE.

## Production: a single self-contained `.exe` (no technical user needed)

For a non-technical user the cleanest deliverable is **one executable** that needs no
Rust toolchain, no Node, and no `ui/dist` folder on their machine. The UI is **embedded
into the binary** at build time (via `crates/server/build.rs`), and on startup the exe
binds the port and **opens the default browser automatically**:

```powershell
# On a machine with Rust + Node installed, produce the single exe:
.\scripts\build-release.ps1
# -> dist/seedfinder.exe  (a self-contained ~2 MB exe with the UI baked in)
```

Hand that **one file** to the user. They double-click it; it starts the server on
`http://127.0.0.1:8080` and opens their browser. They never install or run anything else.

- `SEEDFINDER_NO_OPEN=1` skips opening the browser (headless/CI).
- If the UI hasn't been embedded (a plain `cargo run`), the server falls back to serving
  `ui/dist` from disk and shows a placeholder page if that's absent too — so development
  never breaks when the UI isn't built.

### HTTP API

| Endpoint | Description |
|---|---|
| `POST /api/search` | JSON `{ dsl, low_start, low_end, high_start, high_end, max_per_candidate, include_biomes }`; responds with a `text/event-stream` of `mode` → `result`s (as found) → `done`, plus feasibility `note`s for impossible queries. |
| `GET /api/tile/{seed}/{tx}/{tz}/{lod}` | A 512-block biome PNG tile (quart resolution, LRU-cached, LOD 0–6). |
| `GET /api/catalog` | The authoritative structure list (keys, biome gates, shared-slot partners) for the UI's route-builder dropdowns. |

### Dev mode (Vite + hot reload against a live server)

Run `cargo run -p server`, then in `ui/` run `npm run dev`; Vite proxies `/api` to
`127.0.0.1:8080`.

## CLI

The search engine is drivable offline via the `be-search` binary (no server, no browser,
deterministic):

```bash
# Static feasibility pre-check — proves impossible queries instantly, with reasons
cargo run -p be-search -- feasibility \
  'desert_pyramid a @origin <= 2000
   jungle_pyramid  b @a     <= 400'

# Phase A (structural) then Phase B (biome) search
cargo run -p be-search -- search \
  'village v1 @origin <= 800
   desert_pyramid t1 @v1 in 600..1200, biome=desert' \
  --low-start 0 --low-end 100000 --high-start 0 --high-end 50

# Structure-only (skip the biome pass)
cargo run -p be-search -- search 'village v1 @origin <= 800' --no-biomes
```

`be-corpus` reproduces the ground-truth accuracy report offline:

```bash
cargo run -p be-corpus -- report fixtures/corpus-1.21.40.json
```

## The query DSL

One statement per line (`#` comments and blank lines ignored):

```
<structure> <var> @ <anchor> <range> [; biome=<b1>,<b2>]
```

- `<structure>` — a version-table key (`village`, `desert_pyramid`, …) or `biome` for a
  biome-presence probe.
- `<anchor>` — `origin` or a **previously declared** variable name.
- `<range>` — `<= N`, `>= N`, or `in A..B` (inclusive blocks).

```text
# An Indiana-Jones adventure
village          v1 @origin <= 800
desert_pyramid   t1 @v1 in 600..1200, biome=desert
ocean_monument   m1 @t1 <= 1500
woodland_mansion x1 @origin >= 3000
```

The web UI's route builder compiles to this DSL and can load it back (round-trip), so
you can start visually and graduate to text without losing work. The DSL is parsed by the
same engine in both the CLI and the server.

## Validation & ground truth

Ground truth is **real, captured, and never faked** — it comes from real Bedrock servers
running as Docker containers on a remote host (see `AGENTS.md` for management).

- **Structure placement: 100%** (6 structures × 5 seeds) — `be-struct` predicts the real
  BDS server's `/locate structure` output exactly, across the 1.21.x–1.26.43 range.
- **Biome gate: GREEN at 100%** for the corpus's surface biomes — cubiomes matches the
  real BDS `/locate biome` output at every observed coordinate, on both a matched-version
  **1.21.40** server and the live **1.26.43** server. (An earlier "18.2%" reading was a
  y/z argument-order bug in the cubiomes bridge, now fixed and regression-tested.)

Remaining honesty caveats (also tracked in `PLAN.md` §8):

- Biome parity is **empirically observed, not source-proven** — validated for the
  corpus's surface biomes only; other biomes/edge cases are unproven.
- The shared-salt "scattered" set and a few unresolved source conflicts (e.g. trial
  chambers distribution) are flagged `[UNCONFIRMED]` in the version tables.
- Ground-truth verification of individual *search results* (Phase C, spawning a real
  server per finalist seed) is wired but not run in CI — it needs a live Bedrock server.

## License

MIT. The vendored cubiomes is also MIT (see `crates/cubiomes-sys/cubiomes/README.md`).
