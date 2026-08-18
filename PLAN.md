# Minecraft Bedrock Adventure Seed Finder — Implementation Plan

> **For the implementer:** this plan is self-contained. It was produced from web
> research into Bedrock world generation internals, and every load-bearing technical
> fact below carries a confidence marker. **Facts marked `[UNCONFIRMED]` must be
> verified in Phase 0 before code depends on them.** Do not silently upgrade an
> unconfirmed assumption into an implementation detail.

---

## Current status (progress ledger)

**Updated 2026-08-12.** This section is the live progress ledger for the plan; the
phase sections below describe design intent and are annotated with completion status.

| Phase | Status |
|---|---|
| **0** — De-risk / gate | ✅ **Passed.** Structure params, salts, distributions and the `/locate` anchor for the *validated* structures are empirically confirmed against the real server (see §7). **Biome gate is now GREEN at 100%** (see below). **Shared-salt scattered set (desert_pyramid/igloo/jungle_pyramid/swamp_hut) CONFIRMED 100%** (2026-08-18) against the live BDS 1.26.43 server — `fixtures/corpus-scattered-1.21.40.json` (5 seeds × 4 ids), exact placement on every sample; `versions/1.21.40.json` updated to `confidence: high` with a CI regression gate (see §8). |
| **1** — `be-rng` + `be-struct` | ✅ **Complete.** Streaming MT, full version table, feasibility pre-check. |
| **2** — `be-verify` + `be-corpus` | ✅ **Complete.** BDS harness, `/locate` parser, corpus generator, accuracy reporting. *Note the harness divergence below.* |
| **3** — `cubiomes-sys` + `be-biome` | ✅ **Complete; biome validation GREEN.** FFI, Bedrock ID map, query/gate/grid all built and green, and cubiomes output now **validated 100%** against a matched-version 1.21.40 Bedrock server (and the live 1.26.43 server). |
| **4** — `be-search` | ✅ **Complete.** Query IR + text DSL (round-trip), feasibility pre-check with reasons (shared-slot / triangle-inequality / spacing), rarest-first join planner + adaptive mode, nested-loop executor with per-seed memoisation + rayon, invariant re-check, and a streaming `search_range_visit` seam. |
| **5** — `server` + `ui` | ✅ **Complete.** `crates/server` (axum) exposes `POST /api/search` (SSE stream of mode → results-as-found → done, with feasibility reasons surfaced), `GET /api/tile/{seed}/{tx}/{tz}/{lod}` (server-rendered 512-block biome PNG tiles with LRU cache + LOD), `GET /api/catalog` (structure list for the UI), and serves the built UI from `ui/dist`. `ui/` (Vite + React + TS + Tailwind) is a pan/zoom canvas map with progressive LOD, precision rebasing and declustered structure markers, a text DSL editor driving the SSE stream, an ordered route builder that round-trips to/from DSL, a results list, and URL-encoded shareable view/seed/query state. 13 server tests + 31 vitest tests green. **Remaining:** §7 Phase 5 acceptance runs (open browser, confirm mode selection, VERIFIED badges) — these need the canvas UI to be exercised in a real browser, which is manual. |
| **6** — Optimization | 🚧 **Started (partial).** Seed-lookup speedups landed: zero-alloc streaming MT (`first_n_into`), an allocation-light executor sweep, and **SIMD batching across seeds** (`first_n_batched` + `structure_block_pos_batched` + `Engine::search_range_batched` for the single-variable origin-anchored exhaustive case — bit-identical to scalar, ~3× faster Phase A; measured via `be-rng`/`be-search` example benches). Remaining: constellation result caching, GPU compute (see §6). |

**Measured accuracy (from `fixtures/corpus-1.21.40.json`,
`fixtures/biome-corpus-1.21.40.json` and `fixtures/biome-corpus-1.21.40.bds.json`):**

- **Structure positions: 100%** (6 structures × n=5, within 16 blocks; exact match on
  every sample) — reproduced via `be-corpus report`.
- **Biome agreement: 100%** — cubiomes matches the real BDS `/locate biome` output at
  every observed coordinate on both `biome-corpus-1.21.40.json` (captured against the
  live 1.26.43 server) and `biome-corpus-1.21.40.bds.json` (captured against the 1.21.40
  validation container). **This gate is now GREEN.** The earlier "18.2% / RED" reading
  was a y/z argument-order bug in `cubiomes-sys/src/bridge.c` (`getBiomeAt` returned
  `deep_dark`, the deep-cave biome, at every surface coordinate), not a genuine
  Java↔Bedrock divergence. See "Biome validation — current state" below.

**Key divergences from the original plan text:**
1. **Harness is remote Docker-over-SSH, not a local child process.** §4 originally
   specified spawning BDS as a local child process over stdin/stdout. The production
   path actually built is `be-verify/remote.rs`: `ssh → sudo docker exec → docker logs`
   scrape against the project's Dockerized server (`mc-bedrock`, per AGENTS.md). The
   local `harness.rs`/`fake_bds` remain as a testable abstraction, but they are not the
   production transport. §4 is annotated accordingly below.
2. **Version-provenance honesty.** The corpus and version tables are labeled `1.21.40`,
   but the fixture data was captured against the **live 1.26.43 server** (the only
   server available). Structure params match 100% across that range, so the label is
   practically sound but **provably unverifiable without a 1.21.x server** (flagged
   `UNVERIFIED`). See §4 "1.21.40 validation server" below.

**Biome validation — current state (RESOLVED, gate GREEN):**
- cubiomes caps at **`MC_1_21`** (`MC_NEWEST`); the vendored copy already has the newest
  constants. The live server was **1.26.43** — a five-version gap in which Bedrock biome
  generation could have changed, which made the earlier low agreement ambiguous (version
  drift vs Java↔Bedrock parity).
- **cubiomes is retained.** Research (2026-08-12) found no newer cubiomes and **no
  permissive (MIT/BSD) Bedrock-native biome generator to swap in** — every Bedrock-native
  option is unlicensed or GPLv3, and `Earthcomputer/bedrockified` is a stale (1.13.1)
  Java *mod*, not a biome-ID library. cubiomes is the only MIT, fast, per-seed biome
  source.
- **The decisive experiment was run (2026-08-12).** Stood up an ephemeral **1.21.40.03
  Bedrock server** (matching both the v1 target and cubiomes' max) via
  `be-corpus generate-biome` + `report-biome`. Result: **100% agreement** on the new
  matched-version corpus (`biome-corpus-1.21.40.bds.json`). Re-checking the *original*
  1.26.43 corpus also gave **100%** once the bridge bug was fixed.
- **Root cause of the earlier RED gate: our own bridge bug, not version drift and not
  Java↔Bedrock divergence.** `cubiomes-sys/src/bridge.c` called
  `getBiomeAt(g, scale, x, z, y)`, but cubiomes' signature is `(g, scale, x, y, z)` —
  y/z were swapped, so every surface query returned `deep_dark` (the deep-cave biome).
  Fixed to sample at y=63 (sea level) and regression-tested
  (`cubiomes_sys::surface_biome_is_not_deep_dark`; `be-corpus::biome_parity_gate_is_green`).
- **Conclusion:** cubiomes is validated against real Bedrock (1.21.40 matched version)
  at **100%** for the surface biomes in the corpus. Biome results may be presented as
  Bedrock-accurate for those biomes. The ephemeral 1.21.40 container was removed after
  the run (per the at-most-two-containers rule).

**Next work:** Phase 5 §7 acceptance runs in a real browser (mode selection, VERIFIED
badges, map rendering) which are manual, then the remaining Phase 6 optimization levers
(constellation result caching; GPU compute). SIMD batching across seeds — the init-chain
bottleneck — has been landed (~3× on the single-variable exhaustive sweep). The
1.21.40 biome-validation server was stood up (§4), the biome gate is GREEN
at 100%, and the ephemeral container removed — Phase B biome gates are built on validated
output. **2026-08-18 (validation-widening session):** the anchor corpus was widened from
5 to 10 seeds (47 samples) and the scattered corpus from 5 to 7 seeds (28 samples), both
still 100%; Phase C (`verify-seeds`) was run on 5 real search-result seeds with 0 FAIL.
Mansion / ocean_ruin / trail_ruins and scattered-type resolution turned out **not
validatable via `/locate`** (terrain-dependent generation failure / bounding-box-centre
responses / no per-coordinate biome query) — documented honestly in §8 and
`versions/1.21.40.json` rather than overclaimed. See the session handoff at the end.

---

## Context

Friends want an "Indiana Jones"-style adventure run in **Minecraft Bedrock Edition**.
That needs a world seed where several specific structures (temples, monuments,
mansions) sit close together, in the right biomes, near where they start — a
combination that essentially never occurs by chance and cannot be found by hand.

This project builds a tool that takes a **constraint graph**, **searches the Bedrock
seed space** for satisfying seeds, renders each hit as a **2D pan/zoom minimap**, and —
critically — **verifies finalists against the real game** before presenting them.

Queries range from simple:

> a Desert Pyramid and a Jungle Temple, both within 1500 blocks of origin

…to full multi-leg routes, which is the shape that actually matters:

> start near a Village → Desert Pyramid 600–1200 blocks from it → Ocean Monument within
> 1500 of the pyramid → Woodland Mansion at least 3000 blocks out as the payoff

**The second shape drives the design.** Every constraint in it is relative to another
structure rather than to origin, so the query model is a graph and the engine is a join
planner over that graph (§3.3). A flat, origin-anchored constraint list cannot express
it, and building for flat first would mean rewriting the search core later.

Two properties matter as much as the feature set:

1. **Honesty about accuracy.** Bedrock world generation is undocumented; every
   reimplementation is a reverse-engineering approximation. A seed the tool promises
   has a temple at (412, −880) that turns out empty is worse than no tool. So results
   are validated against real Bedrock worlds and carry a **measured accuracy figure**.
2. **Not brittle.** Deterministic core, heavy unit + property + integration testing,
   and a regression corpus that fails loudly when generation logic drifts.

---

## 1. Settled decisions

These were decided with the project owner. Do not relitigate them without asking.

| Decision | Choice | Status |
|---|---|---|
| **Search origin** | World coordinate **(0,0)**. Bedrock's true spawn is a separate, unreliable RE problem (algorithm changed in 1.21.60; every existing tool admits inaccuracy). Real spawn is always near 0,0. | ✅ Decided; not yet exercised (spawn not needed in v1). |
| **Biome filtering** | **Both** modes: per-structure biome gates, and standalone "biome present within radius". | ✅ Decided; gates/grids built (§3 Phase 3), biome truth **GREEN** (§ "Current status"). |
| **Version support** | **Pluggable version tables as data.** v1 populates and validates **1.21.x** only. | ✅ Implemented (`versions/*.json`). Validation target set to a dedicated **1.21.40** server (§4). |
| **Stack** | **Rust core + embedded local web UI.** Single `.exe`, axum REST+SSE, static canvas UI, opens default browser. | ⬜ Phase 5 not started. |
| **C interop** | **Hybrid** — reimplement structure math in pure Rust; FFI to cubiomes (MIT) for biomes. | ✅ Implemented (`be-struct` pure Rust; `cubiomes-sys` + `be-biome`). |
| **Verification** | **Auto-verify finalists** against Bedrock Dedicated Server before display. | ✅ Remote Docker-over-SSH driver built (§4 divergence); Phase C wiring is Phase 4 work. |

---

## 2. The load-bearing technical facts

### 2.1 The seed splits in half — this is the key insight

**[CONFIRMED, 3 independent sources]** Bedrock has had **64-bit seeds since 1.18.30**
(older sources claiming "Bedrock seeds are 32-bit" are outdated — but see the nuance,
which is what makes them half-true). Structure placement consumes only the **low 32
bits** — the MT seeding does `mt[0] = seed & 0xFFFFFFFF`. Biome and terrain generation
use the **full 64 bits**.

```
seed64 = [ high 32 bits: biomes only ][ low 32 bits: structures + biomes ]
```

Consequences that shape everything downstream:

- **Structure geometry is exhaustively searchable** over 2³² — live exhaustive
  structure search is viable, scoped to the half that matters.
- **The high 32 bits are a second, independent search dimension**, used to satisfy
  biome constraints *after* structural geometry is locked in.
- This is exactly why seed-crackers recover low bits from structures, then need
  biomes to pin the high bits.

### 2.2 Cost asymmetry dictates filter ordering

**[CONFIRMED]** cubiomes' author on biome-validity checks: *"many microseconds instead
of nanoseconds."* Published benchmarks: stock cubiomes **~5.7 µs/query**, optimized
**~0.27 µs**; structure position math is **nanoseconds**. `DoublePerlinNoise` is called
20+ times per biome query; `samplePerlin` alone is 66% of runtime.

**≈1000× lever. Biome evaluation goes last, always.**

### 2.3 The streaming MT19937 optimization is real and published

**[CONFIRMED]** Bedrock uses standard **MT19937** (constants `0x9908b0df`,
`1812433253`/`0x6c078965`, temper masks `0x9d2c5680`/`0xefc60000`, shifts 11/7/15/18) —
not Java's 48-bit LCG. Two published optimizations exist:

- **Partial init, full twist**: initialize only indices `0 ..= min(623, n+396)`.
  Exact, because producing the first `n` tempered outputs touches at most `mt[n+396]`.
- **Full streaming**: never materializes the 624-word array — rolls a scalar forward
  to index 397, computes only `n` twisted words. Working set is a handful of registers.

Callers need **n=2** (linear structures) or **n=4** (triangular). ~400 steps instead of
624. **Prefer the second variant** — its tiny working set is what makes wide SIMD and
GPU viable later.

### 2.4 Region seed formula

**[CONFIRMED, 3 sources agree arithmetically]**

```
regionSeed = (worldSeed + regX*341873128712 + regZ*132897987541 + salt) mod 2^32
mt[0] = regionSeed & 0xFFFFFFFF
regX  = floorDiv(chunkX, spacing)          // floor, not trunc — negatives matter
```

Sampling, with **exact draw order** (getting this wrong is silent corruption):

```
linear     (n=2): x = mNextInt(range); z = mNextInt(range)
triangular (n=4): x1,x2,z1,z2 = four draws IN THAT ORDER
                  x = (x1+x2)>>1 ; z = (z1+z2)>>1
blockPos = ((regX*spacing + chunkInRegionX) << 4) + 8
```

⚠️ `mNextInt(n)` is `next() & (n-1)` for powers of two, else **plain `next() % n` with
no rejection sampling**. It is *biased*, and the bias must be reproduced exactly.

### 2.5 Structure parameters (v1 table, Bedrock 1.21.x)

`separation = spacing − chunkRange`.

| Structure | Salt | Spacing | ChunkRange | Dist. |
|---|---|---|---|---|
| Desert Pyramid / Igloo / Jungle Pyramid / Swamp Hut | **14357617** (shared) | 32 | 24 | Linear |
| Village (1.18+) | 10387312 | 34 | 26 | Triangular |
| Ocean Monument | 10387313 | 32 | 27 | Triangular |
| Woodland Mansion | 10387319 | 80 | 60 | Triangular |
| Pillager Outpost | 165745296 | 80 | 56 | Triangular |
| Ancient City | 20083232 | 24 | 16 | Triangular |
| Ruined Portal (OW) | 40552231 | 40 | 25 | Linear |
| Buried Treasure | 16842397 | 4 | 2 | Triangular |
| Shipwreck (1.18+) | 165745295 | 24 | 20 | ? |
| Ocean Ruin (1.18+) | 14357621 | 20 | 12 | ? |
| Trail Ruins | 83469867 | 34 | 26 | ? |
| Trial Chambers | 94251327 | 34 | 22 | ? |

Notable divergences from Java Edition (do **not** reuse Java values): Pillager Outpost
is 80/56 triangular in Bedrock vs 32/24 linear in Java; Ruined Portal salt differs;
Buried Treasure differs in salt, spacing and distribution; Village and Ancient City
share Java's spacing but use **triangular** where Java uses linear.

**🔑 The shared-salt gotcha, and it hits the adventure use case directly.** Desert
Pyramid, Igloo, Jungle Pyramid and Swamp Hut all share salt `14357617` *and* spacing
32. In Bedrock these are a single "RandomScattered" feature disambiguated **purely by
biome**. So within any given 512×512-block region there is exactly **one**
scattered-structure slot, and its type is decided by the biome that lands on it.

For temple-hunting this means **two different temples can never be closer than one
region apart**, and a request for three scattered structures inside a tight radius may
be outright unsatisfiable. The planner must detect this and tell the user up front
rather than burning ten minutes proving it empirically.

*Confidence: medium (two sources). Surprising enough that Phase 0 verifies it in-game.*

### 2.6 Biome gating makes structures a cheap biome probe

**[CONFIRMED]** Placement runs a biome-validity check over a rectangular region at 4×4
resolution. Desert Pyramid ⇒ `{desert, desert_hills, desert_lakes}`; Jungle Pyramid ⇒
`{jungle, jungle_hills}`; Swamp Hut ⇒ `{swamp, swampland}`; Igloo ⇒
`{icePlains, coldTaiga}`; Mansion ⇒ `{roofedForest}`; Monument ⇒ deep oceans only.

So *"a swamp hut generated here"* ⇒ *"this is swamp."* Usable as a cheap biome proxy,
with two caveats: the inference does **not** run backwards (no hut ≠ no swamp), and
**[UNCONFIRMED]** exactly which coordinate is biome-tested (chunk origin vs. chunk
centre vs. final structure origin, and whether multiple points are sampled). Phase 0
must settle this.

### 2.7 Licensing — reimplement, don't copy

| Project | License | Usable? |
|---|---|---|
| `Cubitect/cubiomes` | **MIT** | ✅ Link + vendor (biomes) |
| `Earthcomputer/bedrockified` | **MIT** | ✅ Cite as provenance (stale: 2019, Java 1.13) |
| `bedrock-dev/MCBEStructureFinder` | **NONE** | ❌ Read for facts, never copy |
| `MZEEN2424/ChunkBiomesGUI` | NOASSERTION | ❌ Also a port of closed-source Chunkbase |
| `Alist2930/MCBE-seedcracker` | NONE | ❌ Useful as an oracle only |
| `FragrantResult186/cubiomes-viewer-bedrock` | GPLv3 | ❌ Viral; differential-test against it only |

Note: some sources report MCBEStructureFinder as MIT. The GitHub API — which is
authoritative — reports **no license**. No license means all rights reserved.

**Policy: constants and algorithms are facts and may be used; expression may not.**
Every constant in the version tables carries a `provenance` field naming its source and
confidence. No source file is copied or mechanically translated.

### 2.8 Known-hard cases (a pre-written test suite)

From `cubiomes-viewer-bedrock`'s own README admissions: Desert Pyramids, Jungle Temples
and Woodland Mansions can **fail to generate in 1.18+ on unsuitable terrain**, because
terrain height is estimated rather than truly generated. These become explicit
low-confidence flags in the UI, not silent wrong answers.

**[UNCONFIRMED — deliberately out of v1 scope]** Bedrock **strongholds** do not use
Java's 3-ring system: random placement, ≥160 blocks out, uncapped count, plus 3
guaranteed under village meeting points. Algorithm not recovered. Ship without it.

Unresolved source conflicts for Phase 0 to settle:
- Nether structure salt: `30084232` vs `430084232`
- Shipwreck ≤1.17 salt: `165745295` vs `1`
- Distribution type (linear vs triangular) for Trial Chambers and Trail Ruins
- Whether Bedrock ravine generation genuinely uses a Java LCG (one source suggests so;
  likely a leftover Java fallback rather than real Bedrock behaviour)

---

## 3. Architecture

```
seedfinder/
├─ crates/
│  ├─ be-rng/        MT19937 + streaming variants + mNextInt bias         [pure Rust]
│  ├─ be-struct/     region math, placement, version tables, feasibility  [pure Rust]
│  ├─ cubiomes-sys/  vendored cubiomes (MIT), cc-built FFI bindings       [C FFI]
│  ├─ be-biome/      safe biome API over cubiomes-sys + Bedrock ID map
│  ├─ be-search/     constraint model, rarest-first planner, rayon engine
│  ├─ be-verify/     BDS child-process harness, /locate parser
│  ├─ be-corpus/     fixture format, accuracy computation + reporting
│  └─ server/        axum REST + SSE; embeds ui/dist via rust-embed
├─ ui/               TypeScript + canvas (tiles, layers, query builder)
├─ versions/         1.21.x.json — structure params, biome gates, provenance
└─ fixtures/         BDS-derived ground truth corpus (checked in)
```

**Version tables are data, not code** (`versions/*.json`), satisfying pluggability:

```jsonc
{ "version": "1.21.40", "seed_bits": 64,
  "structures": {
    "desert_pyramid": {
      "salt": 14357617, "spacing": 32, "chunk_range": 24,
      "distribution": "linear",
      "biomes": ["desert", "desert_hills", "desert_lakes"],
      "shares_slot_with": ["igloo", "jungle_pyramid", "swamp_hut"],
      "provenance": "ChunkBiomesGUI Bfinders.c + MCBEStructureFinder; confidence: high",
      "confidence": "high"
    }}}
```

### 3.1 Search pipeline

```
Phase A — structural sweep over low 32 bits          [ns/check]
  COMPILE ONCE per query: constraint graph → join order (see §3.3)
  for low32 in 0..2^32 (rayon-partitioned, SIMD-batched):
    bind variables in planned order (nested-loop join):
      window = intersection of constraints on already-bound vars
      for regions overlapping window:
        streaming MT → offset → block pos
        satisfies all edges to bound vars? → bind, recurse
      exhausted with no binding → BACKTRACK / reject seed
    → emit structural candidate (geometry only, all vars bound)

Phase B — biome resolution over high 32 bits          [µs/check, satisficing]
  for each candidate low32, stream high32 values:
    full 64-bit seed → cubiomes
    check per-structure biome gates at fixed positions
    check area-presence constraints
    → emit seed candidate

Phase C — ground-truth verification                   [seconds/seed, finalists only]
  spawn BDS with seed → /locate each structure → diff vs prediction
  → emit VERIFIED result (or discard + record the miss as corpus data)
```

**Rarest-first ordering generalises into join ordering.** The planner estimates
`P(pass)` per variable — `P(structure in window | spacing) × P(biome gate)` — and binds
the most selective variable first. This *is* rarest-first, extended to a graph. Biome
rarity is measured empirically at build time by sampling N random seeds and cached,
deliberately avoiding dependence on any unlicensed rarity table.

**Relative constraints make the search faster, not slower.** Once variable A is bound,
an edge "B within 1500 of A" yields a far smaller region window than "B within 1500 of
origin" — and each additional edge prunes harder. Expressiveness buys speed here, which
is the opposite of the usual trade.

**⚠️ Completeness is adaptive, and the UI must not lie about it.** Full 2⁶⁴ enumeration
is impossible; this engine *satisfices*. Beyond that:

| Query shape | Mode | Guarantee |
|---|---|---|
| 1–2 variables, origin-anchored | Exhaustive sweep of all 2³² low halves | Complete over the structural subspace |
| 3+ variables, relational edges | Satisficing — sample low32 until enough hits | **None.** Absence of results ≠ no such seed |

A 5-waypoint chain costs roughly 10⁵–10⁶ operations per seed; times 2³² that never
finishes. The planner estimates per-seed cost at compile time, picks the mode, and
**displays which mode is running**. Never imply completeness the engine cannot deliver.

**Feasibility pre-check — static analysis of the constraint graph**, run before any
search and in milliseconds:

- **Shared-slot conflicts** (§2.5) — "Desert Pyramid within 400 blocks of Jungle Temple"
  is *provably impossible*: they share one placement slot every 512 blocks.
- **Triangle inequality** over distance edges — `d(A,B) ≤ 500 ∧ d(B,C) ≤ 500 ∧
  d(A,C) ≥ 2000` is unsatisfiable regardless of seed.
- **Spacing-vs-radius bounds** — a structure with spacing 80 (1280 blocks) cannot appear
  twice within 1000 blocks.

Report the *reason*, not just "no results". This is a headline feature: with a graph
query language, users will write impossible queries for non-obvious reasons, and proving
it instantly beats failing after a ten-minute search.

**Per-seed memoisation.** Multiple constraints query the same (structure_type, region)
during one seed's evaluation. Memoise within the seed's scope — cheap, and it compounds
with the number of variables.

**Performance shape.** Simple exhaustive queries: tens of seconds for a full sweep,
first hits in well under a second. Complex relational queries: hits stream continuously
with no completion point. Either way results go to the UI over SSE as found — never
block on a completed sweep.

### 3.2 Map rendering

Server-side tiles, which avoids WASM entirely — a simplification available only because
this is localhost.

- **Fragment/tile model** (Amidst's architecture): 512-block tiles, biomes sampled at
  quart (1:4) resolution, keyed `(version, seed, dim, x, z, lod)`.
- Rust renders tiles to PNG; canvas blits them. LRU cache, keyed by seed+version.
- **Progressive LOD**: stretch a coarse tile immediately, swap in the sharp one when
  ready. This is what makes it *feel* fast.
- Structure icons as a separate overlay layer with zoom-dependent declustering
  (Chunkbase's weakest point — icons clump illegibly at low zoom).
- Camera origin periodically rebased: MC coordinates reach ±30M and float32 canvas
  transforms lose precision at deep zoom.
- URL-encoded state so seeds and views are shareable with the group.

### 3.3 Query model — a constraint graph

Queries are **graphs, not flat lists**. An adventure is a *route*, and every interesting
constraint in a route is relative to another structure rather than to origin. A
flat origin-anchored model cannot express "Desert Pyramid 600–1200 blocks from the
village", which is the single most common thing users will want.

- **Nodes** = structure instances (variables to bind)
- **Edges** = distance constraints: unary (to origin) or binary (between two variables),
  each a **min–max range**, not just a max
- Nodes may carry a biome gate; standalone "biome present within radius" remains a
  node type

```rust
struct Query { vars: Vec<Var>, edges: Vec<Edge>, version: Version }
struct Var  { name: String, kind: VarKind, biome: Option<BiomeSet> }
enum VarKind { Structure(StructureType), BiomePresence(BiomeSet) }
struct Edge { from: Anchor, to: VarRef, range: Range<u32> }  // Anchor = Origin | VarRef
```

**v1 constraint vocabulary — deliberately scoped:**

| Constraint | Status |
|---|---|
| Relative distance between structures (min–max range) | ✅ **In v1** |
| Distance to origin (min–max range) | ✅ In v1 |
| Per-structure biome gate | ✅ In v1 |
| Biome present within radius | ✅ In v1 |
| Negative / exclusion (`not X within d of Y`) | ⛔ Deferred |
| Counts / multiplicity (`at least 3 villages`) | ⛔ Deferred |
| Route geometry (collinearity, leg bounds, total length) | ⛔ Deferred |

**Reserve grammar and IR space for the deferred three** so adding them later is additive
rather than a breaking change. Exclusion in particular inverts the search — absence must
be proven across a window rather than found — so the planner should be written with that
shape in mind even while it is unimplemented.

**Two authoring surfaces over one IR.** Both compile to the same `Query`, so the engine
never knows which was used:

*Route builder* — the default UI. An ordered waypoint chain, which is the natural shape
of an adventure and is very hard to mis-specify (this is where cubiomes-viewer's freeform
condition tree hurts users). Covers the large majority of real queries.

```
1. [Village        ▾]   within 800 of origin
2. [Desert Pyramid ▾]   600–1200 from ①      biome: desert
3. [Ocean Monument ▾]   ≤ 1500 from ②
4. [Mansion        ▾]   ≥ 3000 from origin
```

*Text DSL* — the escape hatch, for arbitrary graphs the linear builder can't express
(e.g. three temples all mutually within 2000 blocks). It is version-controllable,
diffable and **unit-testable**, which directly serves the not-brittle goal: every
example query becomes a regression test, and the DSL corpus doubles as the parser suite.

```
village        v1  @origin <= 800
desert_temple  t1  @v1 in 600..1200, biome=desert
monument       m1  @t1 <= 1500
mansion        x1  @origin >= 3000
```

The route builder must round-trip to DSL and back, so users can start visually and
graduate to text without losing work.

---

## 4. Ground truth & accuracy

**No usable public Bedrock seed corpus exists** — research found only unstructured,
version-ambiguous, self-admittedly unverified prose seed lists. Build our own.

**Harness** (`be-verify`) — **original design:** official Bedrock Dedicated Server (free
Windows download) spawned as a child process, commands over **stdin**, results scraped
from **stdout**. Bedrock has **no RCON** — treat any tutorial claiming otherwise as
wrong. One fresh world per seed via `LEVEL_SEED`.

> **✅ Superseded — production harness is remote Docker-over-SSH.** The path actually
> built is `be-verify/remote.rs`: it shells out to `ssh -l <user> <host>` then
> `sudo docker exec mc-bedrock ...`, sending commands through the `itzg` image's
> `send-command` helper and scraping responses from `docker logs` (see AGENTS.md). The
> `harness.rs`/`fake_bds` local child-process abstraction still exists and is the
> testable seam (integration test `fake_bds_integration.rs`), but it is **not** the
> production transport. The "one fresh world per seed" model is achieved by `sed`
> `level-seed=<seed>` into `/data/server.properties`, `rm -rf` the world under
> `/data/worlds`, and restarting the container — see `RemoteBedrock::recreate_world`.
> This driver is gated behind an explicit `--host` flag in `be-corpus generate` and is
> never invoked by unit tests.

**1.21.40 validation server (biome gate).** The live `mc-bedrock` container runs
**1.26.43**, which cannot validate cubiomes' ≤1.21 output. To run the decisive
matched-version experiment (§ "Current status"), we will stand up a **second container
pinned to BDS 1.21.40** on a **different host port** (e.g. 25580), on the same remote
host. To conserve resources on `ai-assistant-01`, **run at most two Minecraft
containers at once** — so the 1.21.40 container should be started for the validation
run and stopped (or removed) when not in use; it is an ephemeral validation tool, not a
permanent server. `be-corpus generate-biome`/`report-biome` then targets that container
via its `--container`/port, and the resulting biome corpus is version-labeled honestly
as captured (see version-provenance note below).

> **RESOLVED 2026-08-12.** This experiment was run: an ephemeral **1.21.40.03** container
> (own volume `mc-bedrock-12140-data`, host port 25580, `ALLOW_CHEATS=true`) captured
> `fixtures/biome-corpus-1.21.40.bds.json`, giving **100%** agreement — and the original
> 1.26.43 corpus also gives **100%** once the bridge y/z bug (§ "Current status") is
> fixed. The container and its volume were removed afterward, returning the host to its
> two permanent servers. Note the image `VERSION` must be the 4-part `1.21.40.03`
> (bare `1.21.40` 404s) and `ALLOW_CHEATS` must be lowercase `true`.

Landmines to design around:

- **Build the stdout parser from captured real output, not from docs.** Message
  punctuation varies between versions and between wiki write-ups.
- **`/locate` has no origin argument in Bedrock.** Syntax is
  `/locate structure <id> [useNewChunksOnly]` and `/locate biome <id>`; it searches from
  the executor's position, i.e. world origin from the console. Fine here (we search
  from 0,0 anyway) but it caps us at one sample per (seed, structure).
- **`/locate structure temple` conflates** desert pyramid / jungle temple / igloo /
  witch hut into one ID. Disambiguate by reading the biome at the returned coordinate —
  the four gate on mutually exclusive biomes.
- **Biome namespace requirement changed at 1.21.100** (`minecraft:plains` vs `plains`).
  Command generation must be version-aware.
- **`/locate` anchor vs centre.** **Resolved for the validated structures:** the corpus
  shows **exact** match (mean/max offset 0.0, n=5 each) for village, ocean_monument,
  ancient_city, pillager_outpost, shipwreck, buried_treasure, ruined_portal — i.e. for
  these, `/locate` returns the structure's block anchor as predicted. **[UNCONFIRMED,
  still open]** for structures not in the corpus (notably **trial_chambers**, which the
  code explicitly excludes because it returns its bounding-box centre — see
  `be-corpus/src/main.rs`). Treat the anchor-vs-centre question as open per-structure
  until each is in the corpus.

**Dense biome truth** comes from a second channel: generate the world, then read the
LevelDB `Data3D` record (tag 43 / 0x2B) biome palettes offline. That yields per-cell
biome grids rather than `/locate biome`'s single nearest match. Note Bedrock uses
Mojang's *fork* of LevelDB — vanilla bindings often fail; use a Mojang-variant-aware
wrapper. Structures stay on `/locate`; LevelDB has no general structure index.

**Surfaced accuracy.** CI runs the corpus and computes per-structure, per-version
agreement. The UI shows it inline — *"Desert Pyramid: 97.3% verified (n=412, 1.21.40)"* —
and flags the §2.8 known-hard structures explicitly. A drop below threshold fails CI.

---

## 5. Testing

| Layer | What |
|---|---|
| **Unit** | MT19937 vs the canonical `mt19937ar.c` reference vectors. Region-seed formula incl. **negative floor-division**. `mNextInt` bias — both the power-of-two mask path and the biased modulo path. Draw order for linear vs triangular. |
| **Property** | Streaming MT output ≡ full MT for the first *n* outputs, ∀ seed, ∀ n ∈ 1..8. **This is the single highest-value test in the project** — it is the optimization most likely to be subtly wrong, and it fails silently. |
| **Golden** | Structure positions for pinned seeds vs checked-in fixtures. Regenerating requires an explicit flag. |
| **Invariant** | Every emitted result is re-checked against its own constraints before display — independently of the planner that produced it. This is the safety net for the whole join-ordering layer: a mis-ordered or over-pruning plan cannot leak a wrong result past it. |
| **DSL** | Parse → IR → re-serialise round-trip. Route-builder ↔ DSL round-trip. A corpus of example queries (including every one in this plan) as parser fixtures. |
| **Planner** | Join order must not change the *result set*, only the speed: for small queries, assert the planned search and a naive brute-force search return identical seeds. Feasibility pre-check tested against known-impossible queries (shared-slot, triangle-inequality violations) — must reject with the correct *reason*, not just reject. |
| **Integration** | Full query → search → BDS verify, on a small seed set. |
| **Regression** | The corpus, in CI, with the accuracy threshold as a gate. |
| **Differential** | Spot-check against `cubiomes-viewer-bedrock` / Chunkbase outputs. Comparison only — no code, no linking (GPLv3). |

---

## 6. Build order

**Phase 0 — De-risk. Do this before writing the engine.** Verify the shaky
assumptions in-game: shared-salt behaviour (§2.5), biome-check coordinate semantics
(§2.6), `/locate` anchor-vs-centre, and the salt conflicts (§2.8). Capture real BDS
stdout for the parser. Implement MT19937 + region seed + *one* structure (village), and
confirm predictions against BDS.
**Gate: if predictions don't match, stop and re-research. Do not build on a broken
generator — every later phase inherits the error.**

> **✅ Status: structure side passed.** Structure params, salts, distributions,
> shared-salt behaviour, and `/locate` anchor for the validated structures are
> confirmed (100% corpus, §7). **Biome side RED** — see § "Current status".

**Phase 1 — `be-rng` + `be-struct`.** Streaming MT, full version table, feasibility
pre-check. Full unit + property suite.
> **✅ Complete.**

**Phase 2 — `be-verify` + `be-corpus`.** BDS harness, `/locate` parser, corpus
generator, accuracy reporting. *Deliberately early — it is what makes every later phase
trustworthy.*
> **✅ Complete** (harness is the remote Docker-over-SSH driver — see §4 divergence).

**Phase 3 — `cubiomes-sys` + `be-biome`.** Vendor cubiomes, FFI, Bedrock biome ID
mapping, validate against LevelDB `Data3D` grids.
> **✅ Code complete; biome validation RED.** FFI, ID map, query/gate/grid built and
> green (16 tests). Validation is blocked on server version (see § "Current status");
> the planned LevelDB `Data3D` channel is **not yet implemented** — biome truth
> currently comes from `/locate biome` alone.

**Phase 4 — `be-search`.** The largest phase; build it in this order:
1. `Query` IR + DSL parser (IR first — both authoring surfaces target it).
2. Feasibility pre-check / static analysis, with reasons.
3. Join planner: selectivity estimation and ordering.
4. Nested-loop bind-and-backtrack executor, per-seed memoisation, rayon.
5. Adaptive mode selection (exhaustive vs satisficing) + honest mode reporting.
6. SSE streaming, Phase C verification wired in.
> **🚧 Items 1–5 complete.** `be-search` implements the IR, DSL (plan-example round-trip +
> three-temples graph fixture), feasibility (`shared_slot_conflict_is_detected`,
> `triangle_inequality_violation_is_detected`, `same_structure_too_close_is_detected`),
> planner (`rarest_structure_binds_first`, `relational_query_is_satisficing`), and the
> executor with `sequential_and_parallel_agree` + planner-vs-brute-force invariant
> (`tests/planner_invariant.rs`). Item 6 (SSE + Phase C) is wired in Phase 5 when the
> server exists; the executor's `search_range`/`search_range_par` are the streaming seam.
> The 1.21.40 validation server was stood up and removed (§4), so Phase B biome gates are
> built on validated cubiomes output.

**Phase 5 — `server` + `ui`.** REST/SSE, tile renderer, canvas map, **route builder**
with live feasibility feedback, DSL editor with round-trip, accuracy display.
> ✅ **Complete (2026-08-12).** `crates/server` implements the axum REST+SSE layer, the
> streaming search (`POST /api/search`), the server-side 512-block biome tile renderer
> with LRU cache + LOD (`GET /api/tile/...`), the structure catalog endpoint
> (`GET /api/catalog`), and static serving of the built UI from `ui/dist`. `ui/`
> (Vite + React + TS + Tailwind) provides the pan/zoom canvas map (tiles + progressive
> LOD + precision rebase + declustered structure markers), the text DSL editor driving
> the SSE stream with honest mode/completeness reporting, an ordered route builder that
> round-trips to/from DSL, a results list, and URL-encoded shareable state. The §7
> acceptance runs that require a real browser remain manual.

**Phase 6 — Optimization, only if measured need.** SIMD batching across seeds;
constellation result caching; GPU compute for the Phase A sweep (viable per §2.3 — the
streaming MT's tiny working set means per-thread state is a handful of words, not the
624 that would wreck occupancy).

---

## 7. Verification

1. `cargo test --workspace` — unit, property, golden, invariant. **Green as of 2026-08-12**
   (144 tests, 0 failures across `be-rng`, `be-struct`, `be-verify`, `be-corpus`,
   `be-biome`, `cubiomes-sys`, `be-search`, and `server`).
2. `cargo test --features bds-integration` — **superseded**: the live path is the remote
   Docker-over-SSH driver (`be-verify/remote.rs`), gated behind `--host`. The local
   child-process harness is exercised via `fake_bds` in `cargo test --workspace`
   (4 integration tests), not against a live BDS install.
3. `cargo run -p be-corpus -- generate --version 1.21.40 --seeds <n> --host <host>` then
   `cargo run -p be-corpus -- report fixtures/corpus-1.21.40.json [tolerance]` →
   per-structure accuracy table. **Current fixtures reproduce:**
   ```
   version    structure               n exact <=tol   rate%      mean       max     off.dx     off.dz
   1.21.40    ancient_city            5     5     5   100.0       0.0       0.0        0.0        0.0
   1.21.40    buried_treasure         5     5     5   100.0       0.0       0.0        0.0        0.0
   1.21.40    pillager_outpost        5     5     5   100.0       0.0       0.0        0.0        0.0
   1.21.40    ruined_portal           5     5     5   100.0       0.0       0.0        0.0        0.0
   1.21.40    shipwreck               5     5     5   100.0       0.0       0.0        0.0        0.0
   1.21.40    village                 5     5     5   100.0       0.0       0.0        0.0        0.0
   overall: 100.0%
   ```
   And the biome gate (`cargo run -p be-corpus -- generate-biome ...` then
   `report-biome fixtures/biome-corpus-1.21.40.json`) reproduces **18.2%** overall
   biome agreement — **RED**, the blocker for §8.
4. `cargo run` → browser opens. Run **both** query shapes:
   - *Simple:* Desert Pyramid + Jungle Temple within 1500 of origin → confirm it selects
     **exhaustive** mode and reports completion.
   - *Complex route:* the 4-waypoint chain from §3.3 → confirm it selects **satisficing**
     mode, says so, and streams hits continuously.
   In both cases the map renders and each result carries a **VERIFIED** badge plus an
   accuracy figure.
   > **⬜ Not yet run** — requires Phase 5 (`server` + `ui`).
5. Submit a known-impossible query (Desert Pyramid within 400 blocks of Jungle Temple)
   → must reject in milliseconds **with the shared-slot reason**, not search and fail.
   > **⬜ Not yet run** — requires Phase 4 feasibility pre-check.
5. **End-to-end truth test:** take a returned seed, create a world in *retail Minecraft
   Bedrock* on the target version, and walk to the coordinates. This is the only test
   that actually matters to the people using this.
   > **⬜ Outstanding** — requires a returned (verified) seed, i.e. after Phase 4+5.

---

## 8. Open risks

| Risk | Mitigation |
|---|---|
| Post-1.18 Bedrock↔Java biome parity is **empirically observed, not proven** — no source-level decompilation confirms identical noise parameters, and Mojang's own framing was "not 100% there yet, but close." | This is precisely why the measured accuracy figure is load-bearing rather than decorative. **RESOLVED 2026-08-12 for the corpus's surface biomes: cubiomes matches the real BDS server 100% on both a matched-version (1.21.40) and a 1.26.43 corpus.** The earlier RED reading was a bridge bug, not a parity gap. Remaining scope: biomes/biomes beyond the corpus. Validate against LevelDB grids; surface the number. |
| **Biome gate is RED at 1.26.43** — cubiomes (≤1.21 Java) agrees with the live server only 18.2%, conflating version drift with edition parity. | **RESOLVED 2026-08-12.** The 18.2% figure was caused by a y/z argument-order bug in `cubiomes-sys/src/bridge.c`, not version drift or edition parity. Fixed (sample at y=63) and regression-tested. The matched-version **1.21.40.03** server was stood up, `generate-biome`/`report-biome` re-run → **100%** on both corpora; ephemeral container then removed. Biome results for the corpus's surface biomes may be presented as Bedrock-accurate. |
| Temples can fail to generate on unsuitable terrain in 1.18+ (§2.8) | Flag affected structures as lower-confidence in the UI; BDS verification catches individual misses. |
| Shared-salt finding is only medium-confidence | **RESOLVED 2026-08-18.** The shared-salt scattered set (desert pyramid / igloo / jungle pyramid / swamp hut) was captured against the live BDS 1.26.43 server (`generate-scattered`, 5 seeds × 4 ids) and **confirms 100% exact placement** (`fixtures/corpus-scattered-1.21.40.json`, `be-corpus report`). `versions/1.21.40.json` confidence bumped to `high` with a CI regression gate. |
| **Complex relational queries have no completeness guarantee** — "no results" may mean "none exist" or "not found yet", and users will read it as the former | Display the active mode and elapsed search space prominently. Never render an empty result set as "no such seed exists". |
| Join planner picks a bad order and a query runs pathologically slow | Selectivity estimates are heuristics; add a per-query cost ceiling that triggers re-planning, and surface a "this query is expensive because…" explanation. |
| Expressive queries invite unsatisfiable ones | Static feasibility analysis with explained reasons (§3.1); live feedback in the route builder as constraints are edited, not just at submit. |
| Bedrock updates silently change parameters | Version tables are data; corpus regression fails loudly. |
| BDS stdout format drift | Parser built from captured output, version-gated, with fixtures. |
| **Corpus version label (`1.21.40`) captured against `1.26.43`** — `UNVERIFIED` provenance | Structure params agree 100% across the range (practically sound), and the 1.21.40 validation server (see above) was run: it produced the matched-version biome corpus `biome-corpus-1.21.40.bds.json` (100%) and re-confirmed the biome gate against the correct version. |

---

## Appendix: primary sources

- [Cubitect/cubiomes](https://github.com/Cubitect/cubiomes) (MIT) — biome generation
- [Earthcomputer/bedrockified](https://github.com/Earthcomputer/bedrockified) (MIT) — origin of most public Bedrock structure knowledge
- [bedrock-dev/MCBEStructureFinder](https://github.com/bedrock-dev/MCBEStructureFinder) — best technical reference; **unlicensed, read only**
- [MZEEN2424/ChunkBiomesGUI](https://github.com/MZEEN2424/ChunkBiomesGUI) — most current parameter table; **unlicensed, read only**
- [minecraft.wiki — World seed](https://minecraft.wiki/w/World_seed) · [Bedrock 1.18.30](https://minecraft.wiki/w/Bedrock_Edition_1.18.30) · [Bedrock level format](https://minecraft.wiki/w/Bedrock_Edition_level_format) · [/locate](https://minecraft.wiki/w/Commands/locate)
- [Bedrock Dedicated Server download](https://www.minecraft.net/en-us/download/server/bedrock)
- [Ultrafast Minecraft Biome Generation (benchmarks)](https://rohan-sharma.de/blog/cubiomesmpi-part1/)


---

## Session handoff — 2026-08-18 (SIMD wiring, CI guard, validation tooling)

> Self-contained handoff for a fresh session. This session: (1) wired the SIMD-batched
> sweep into the real CLI + server search paths, (4) added a CI regression guard, and
> (3) built the Phase C + shared-salt validation tooling. #2 (constellation result
> caching) was **deferred**. Commits: `78affbe` (perf wiring + CI guard), `49b7fc6`
> (validation tooling).

### What shipped this session

**#1 — SIMD batching is now the real search path (done).**
- `Engine::search_range_visit_batched` added (`crates/be-search/src/executor.rs`) —
  a streaming seam that emits candidates in ascending seed order, using the batched
  sweep for single-structure origin-anchored queries and transparently falling back to
  scalar `search_range_visit` otherwise. Refactored the batched core into
  `batched_single_var_for_each`.
- Server `POST /api/search` (`crates/server/src/search.rs::run_search`) now uses the
  batched visitor (SSE path).
- CLI `search` (`crates/be-search/src/main.rs::cmd_search`) now uses
  `search_range_batched` for Phase A.
- Tests: batched visitor == scalar visitor sequence (incl. fallback shapes), and a
  server multi-var fallback test. Result set is bit-identical; mode/completeness
  reporting unchanged.

**#4 — CI regression guard (done).**
- `crates/be-search/examples/bench_sweep.rs` gained `--check`: runs the release-mode
  sweeps and exits nonzero if the worst scalar→batched speedup drops below 1.15× (real
  is ~3×). Correctness (batched == scalar) already runs in `cargo test`.
- `.github/workflows/ci.yml` rust job now runs
  `cargo run -p be-search --release --example bench_sweep -- --check`.

**#3 — tooling built (offline-tested); live run is an ops step.**
- `be-corpus verify-seed` (Phase C): fresh world of a returned seed → `/locate` each
  anchor structure → diff the model's region-backed-out placement vs the observation.
- `be-corpus generate-scattered` (shared-salt): capture `/locate structure temple`
  observations and record them under all four shared-salt scattered ids, so `report`
  can validate the scattered set (previously `[UNCONFIRMED]`).
- Pure logic lives in `crates/be-corpus/src/verify.rs` and `src/scattered.rs`
  (offline unit-tested); live I/O is thin and gated behind `--host`.

### Live-run commands (require the remote Bedrock server, AGENTS.md)

These talk to the real `mc-bedrock` container and take real wall-clock time. They are
**not** run in CI. The SSH agent on this machine has the keys.

```sh
# Phase C: verify one returned seed against the real server
cargo run -p be-corpus -- verify-seed --seed 4242 --host ai-assistant-01.longbranch.lwalker.me
#   optional: --structures village,ocean_monument --tolerance 16 --user luser

# Shared-salt: capture a scattered-set corpus from the real server (N seeds)
cargo run -p be-corpus -- generate-scattered --seeds 5 --host ai-assistant-01.longbranch.lwalker.me --out fixtures/corpus-scattered-1.21.40.json
# then, offline, score it:
cargo run -p be-corpus -- report fixtures/corpus-scattered-1.21.40.json
```

**Live validation — DONE 2026-08-18.** Ran against the live `mc-bedrock` (1.26.43) server:
- `generate-scattered --seeds 5` → `fixtures/corpus-scattered-1.21.40.json` (20 samples),
  `report` → **100% exact** on all four scattered ids.
- `verify-seed` on returned seeds 0 and 1 → **0 FAIL**; every structure that responded
  PASSed (the remainder were honest SKIPs — `ocean_monument` genuinely absent near
  origin, others "no parseable response" from docker-log scrape timing).
- `versions/1.21.40.json`: scattered set bumped to `confidence: high` + provenance note;
  Phase 0 caveat cleared; a CI regression gate was added for the scattered corpus.
- Tooling improvement made during the run: `response_wait` 1.5s→4s and a 5s settle after
  world recreation, because the first `/locate` right after a fresh world boot races
  chunk generation (was producing spurious "no parseable response" SKIPs).

### #2 — constellation result caching (deferred, do not pick up yet)

Least-specified item. When you do it: cache computed structure geometry keyed by
`(version, structure, region)` with a bounded eviction policy; must be bit-identical
with the cache off (test cache-hit == recompute). Not a blocker for anything else.

### Discovery worth remembering

The search engine's `region_window` is **not distance-ordered** (row-major). So
`evaluate_seed` does NOT reliably return the *nearest* structure that `/locate`
returns — do not use it to predict `/locate` results for a fresh seed. That is exactly
why `verify-seed` uses the corpus's region-backed-out placement check instead.

### Options for next steps (pick one or more next session)

1. ~~**Run the live validation**~~ **DONE 2026-08-18** (see above): scattered corpus
   captured + reported 100%, version table + Phase 0 caveat updated, Phase C verified
   returned seeds with no failures. (Re-running `generate-scattered`/`verify-seed` is
   still the way to widen the corpus or check new seeds.)
2. **Constellation result caching** (#2) — now that the SIMD work is wired and guarded.
3. **GPU compute** (Phase 6 remaining) — the other documented optimization lever.
4. **UI/acceptance** — §7 Phase 5 manual acceptance runs (browser, mode selection,
   VERIFIED badges).
5. **Broaden validation** — more anchor structures/seeds in the corpus, or validate the
   biome-gated search (which scattered structure appears where) against the live server.
6. **Docs/status** — refresh CLAUDE.md/README Phase 6 wording to note the batched path
   is now live + CI-guarded (currently they say "not wired into the CLI/server").

---

## Session handoff — 2026-08-18 (Phase C verification + validation widening)

This session did the remaining validation work: (1) ran Phase C (`verify-seeds`) on real
search-result seeds against the live server, (2) widened the anchor and scattered corpora,
(3) attempted mansion / ocean_ruin / trail_ruins / scattered-type validation. The honest
outcome is that the reliable structures are confirmed 100%, Phase C passes, and
**`woodland_mansion` was resolved as live-verified** (the earlier "no mansion" was a wrong
`/locate` id — see below), while ocean_ruin, trail_ruins and scattered-type resolution
turned out **not validatable via `/locate`** and are documented as such rather than
overclaimed.

### Code shipped
- `be-corpus verify-seeds` (multi-seed Phase C: `--seeds a,b,c` and/or `--stdin`, decimal
  or `0x`-hex seeds, per-seed PASS/FAIL/SKIP, nonzero exit iff any FAIL). `verify-seed`
  refactored to share `verify_one_seed`.
- `be-search search --seeds-only` (one 64-bit decimal seed per line) → pipes cleanly into
  `verify-seeds --stdin`.
- `be-corpus generate-probe` (separate, non-gated corpus for low-confidence structures)
  and `generate-scattered-type` / `report-scattered-type` (exploratory type resolution).
- `be-corpus::scattered::{predict_scattered_type, primary_gate_biome}` (pure, unit-tested).
- `be-corpus/tests/phase_c_logic.rs` — offline CI gate pinning the Phase C decision logic
  (`compare`/`predict_for_region`/`Verdict`) and that every anchor structure (incl. mansion)
  is modelled.
- `cmd_generate` response_wait 1500ms → 4000ms (the AGENTS.md-known flaky setting).
- `woodland_mansion` added to `ANCHOR_STRUCTURES` and the generate probe list.

### Live validation results (2026-08-18, live BDS 1.26.43)
- **Main corpus widened:** `fixtures/corpus-1.21.40.json` → 10 seeds / **47 samples, 100%**
  (7 anchor structures; ocean_monument honestly absent near origin).
- **Scattered corpus widened:** `fixtures/corpus-scattered-1.21.40.json` → 7 seeds /
  **28 samples, 100%**.
- **Phase C on real search seeds:** `be-search search "village v1 @origin <= 800
  \ndesert_pyramid d1 @v1 in 400..1200" --seeds-only | verify-seeds` on 5 distinct-low32
  seeds → **0 FAIL**; every parseable observation PASSed (SKIPs were honest).
- **Mansion:** LIVE-VERIFIED (2026-08-18). The earlier 4/4 "no mansion" was an artifact of
  the **wrong `/locate` id**: Bedrock's `/locate structure` id is `mansion`, not
  `woodland_mansion` (the model id returns "No valid structure found"). With the correct id,
  the model's placement matches the live BDS 1.26.43 server at **100%** (7/7 resolved seeds,
  region-backed-out exact). Structural params (salt 10387319, spacing 80, chunk_range 60,
  triangular) confirmed correct. The `be-corpus::locate_id` mapping is wired into the Phase C
  verify flow; the search model id stays `woodland_mansion`.
- **ocean_ruin:** `/locate` never resolved a parseable position (6 seeds) — inconclusive.
- **trail_ruins:** returns a variable non-anchor (bbox-centre) position — 0% on the anchor
  model (like trial_chambers) — not confirmable via /locate.
- **Scattered-type resolution:** inconclusive — biome `/locate` is slow/flaky and there is
  no per-coordinate biome query (§2.6). Documented limitation.

### What NOT to claim
Do not present ocean_ruin, trail_ruins, or the scattered-type resolution as live-verified.
(woodland_mansion IS live-verified as of 2026-08-18 — see above.) `versions/1.21.40.json`
and §8 carry the honest UNVERIFIED / low-confidence notes; `fixtures/corpus-probe.json`
records the trail_ruins negative result.

### Still open (unchanged from prior handoff)
- Phase 5 §7 manual browser acceptance runs.
- Phase 6 remaining: constellation result caching; GPU compute.
- **Biome-corpus widening deferred.** Attempting to widen `biome-corpus-1.21.40.json` from
  5 to 8 seeds gave **87.5%** (plains 71%, forest 86%) instead of 100%, but the mismatches
  look like `/locate biome` scrape contamination (e.g. the same nearest-forest coordinate
  recorded for two different seeds, a known stale-response symptom) rather than genuine
  Java↔Bedrock parity divergence. The original 100% corpus is retained as the validated
  fixture; biome widening should only be re-attempted after making the biome `/locate`
  scrape staleness-proof (investigate the `docker logs --since` window under rapid
  sequential locates).
