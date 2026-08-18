//! Regression gate over the checked-in real corpus (PLAN §5 "Regression" layer).
//!
//! `fixtures/corpus-1.21.40.json` was generated against the **real** BDS 1.26.43 server
//! via `be-corpus generate` (the §4 "one fresh world per seed" flow). This test
//! recomputes predictions with `be-struct` and asserts the accuracy stays at the
//! recorded level. It fails loudly if the generator drifts — the whole point of the
//! corpus regression gate.

use be_corpus::{accuracy, Corpus, Version};

const CORPUS_PATH: &str = "fixtures/corpus-1.21.40.json";
const TOLERANCE: u64 = 16;
/// Accuracy below this fails the gate (PLAN §4: drop below threshold fails CI).
const MIN_RATE: f64 = 1.0;

#[test]
fn real_corpus_accuracy_holds() {
    // Resolve the corpus path relative to the crate directory.
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above be-corpus crate");
    let path = base.join(CORPUS_PATH);

    let corpus = Corpus::load(path.to_str().unwrap()).expect("corpus fixture loads");
    assert!(!corpus.is_empty(), "corpus must not be empty");

    let version = Version::builtin_1_21_40();
    let rate = accuracy::overall_rate(&corpus, &version, TOLERANCE)
        .expect("corpus has comparable samples");
    assert!(
        rate >= MIN_RATE,
        "corpus accuracy {:.1}% dropped below gate {:.1}% (generator drift?)",
        rate * 100.0,
        MIN_RATE * 100.0
    );
}

#[test]
fn real_corpus_has_expected_structures() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above be-corpus crate");
    let path = base.join(CORPUS_PATH);
    let corpus = Corpus::load(path.to_str().unwrap()).expect("corpus fixture loads");

    let structures: Vec<&str> = corpus
        .samples
        .iter()
        .map(|s| s.structure.as_str())
        .collect();
    // trial_chambers is intentionally absent (returns bounding-box centre; see
    // be-struct golden.rs). All anchor-returning structures should be present.
    for id in [
        "village",
        "ancient_city",
        "pillager_outpost",
        "shipwreck",
        "buried_treasure",
        "ruined_portal",
    ] {
        assert!(structures.contains(&id), "corpus missing {id}");
    }
    assert!(!structures.contains(&"trial_chambers"));
}

/// Biome parity gate — now **GREEN** and pinned at 100%.
///
/// cubiomes (≤1.21 Java) matches the real BDS server's `/locate biome` output at every
/// observed coordinate, on both captured corpora:
///
/// - `fixtures/biome-corpus-1.21.40.json` — captured against the **1.26.43** server.
/// - `fixtures/biome-corpus-1.21.40.bds.json` — captured against the **1.21.40**
///   validation container (matched version).
///
/// Previously this gate was RED (~18%) because of a y/z argument-order bug in the
/// cubiomes bridge (`getBiomeAt`), which returned the deep-cave biome (`deep_dark`) at
/// every surface coordinate. That bug is fixed (see `cubiomes-sys`
/// `surface_biome_is_not_deep_dark`) and this gate now asserts the correct 100%
/// agreement so any regression fails loudly.
#[test]
fn biome_parity_gate_is_green() {
    use be_biome::{builtin_biome_map, BiomeQuery, CubiomesQuery};

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above be-corpus crate");

    let map = builtin_biome_map();
    let mc = cubiomes_sys::mc_latest();
    let resolve = |name: &str| -> Option<u16> { map.bedrock_id_for_name(name) };

    // Both corpora must agree 100%: the matched-version (1.21.40) one and the
    // originally-captured (1.26.43) one.
    for rel in [
        "fixtures/biome-corpus-1.21.40.json",
        "fixtures/biome-corpus-1.21.40.bds.json",
    ] {
        let path = base.join(rel);
        let corpus = Corpus::load(path.to_str().unwrap())
            .unwrap_or_else(|e| panic!("biome corpus fixture {rel} loads: {e}"));
        assert!(!corpus.biome_samples.is_empty(), "{rel} must not be empty");

        let query_id = |seed: u64, x: i64, z: i64| -> Option<u16> {
            let q = CubiomesQuery::new(mc, seed);
            q.biome_id_at(x as i32, z as i32)
        };

        let rate = accuracy::biome_overall_rate(&corpus, query_id, resolve)
            .expect("biome corpus has comparable samples");

        assert!(
            (rate - 1.0).abs() < 1e-9,
            "biome agreement for {rel} dropped to {:.1}% (bridge regression?) — expected 100%",
            rate * 100.0
        );
    }
}

/// Shared-salt scattered set regression gate (PLAN §2.5, #3).
///
/// `fixtures/corpus-scattered-1.21.40.json` was captured against the live BDS 1.26.43
/// server via `be-corpus generate-scattered` (5 seeds × 4 scattered ids). It pins the
/// shared placement math (salt 14357617, spacing 32, chunk_range 24, linear) for
/// desert_pyramid / igloo / jungle_pyramid / swamp_hut at 100% so any drift fails loudly.
const SCATTERED_PATH: &str = "fixtures/corpus-scattered-1.21.40.json";

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above be-corpus crate")
        .to_path_buf()
}

#[test]
fn scattered_set_accuracy_holds() {
    let path = workspace_root().join(SCATTERED_PATH);
    let corpus = Corpus::load(path.to_str().unwrap()).expect("scattered corpus fixture loads");
    assert!(!corpus.is_empty(), "scattered corpus must not be empty");

    let version = Version::builtin_1_21_40();
    let rate = accuracy::overall_rate(&corpus, &version, TOLERANCE)
        .expect("scattered corpus has comparable samples");
    assert!(
        rate >= MIN_RATE,
        "scattered-set accuracy {:.1}% dropped below gate {:.1}% (shared placement drift?)",
        rate * 100.0,
        MIN_RATE * 100.0
    );
}

#[test]
fn scattered_set_has_all_four_ids() {
    let path = workspace_root().join(SCATTERED_PATH);
    let corpus = Corpus::load(path.to_str().unwrap()).expect("scattered corpus fixture loads");
    let structures: Vec<&str> = corpus
        .samples
        .iter()
        .map(|s| s.structure.as_str())
        .collect();
    for id in ["desert_pyramid", "igloo", "jungle_pyramid", "swamp_hut"] {
        assert!(structures.contains(&id), "scattered corpus missing {id}");
    }
}
