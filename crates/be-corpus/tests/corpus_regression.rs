//! Regression gate over the checked-in real corpus (PLAN §5 "Regression" layer).
//!
//! `fixtures/corpus-1.21.40.json` was generated against the **real** BDS 1.26.43 server
//! via `be-corpus generate` (the §4 "one fresh world per seed" flow). This test
//! recomputes predictions with `be-struct` and asserts the accuracy stays at the
//! recorded level. It fails loudly if the generator drifts — the whole point of the
//! corpus regression gate.

use be_corpus::{Corpus, Version, accuracy};

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
    for id in ["village", "ancient_city", "pillager_outpost", "shipwreck", "buried_treasure", "ruined_portal"] {
        assert!(structures.contains(&id), "corpus missing {id}");
    }
    assert!(!structures.contains(&"trial_chambers"));
}

/// Biome parity gate is **RED** and intentionally pinned at the observed low level.
///
/// cubiomes (≤1.21) does not match the BDS 1.26.43 server: agreement is ~18% (2/11)
/// on `fixtures/biome-corpus-1.21.40.json`. This is version drift (cubiomes caps at
/// 1.21; the server is 1.26.43), not a pipeline bug. This test does NOT assert high
/// agreement — it pins the honest, currently-failing figure so that if a future
/// cubiomes update or a supported-version server raises agreement, the change is
/// noticed and the gate can be flipped green deliberately.
#[test]
fn biome_parity_gate_is_red_and_pinned() {
    use be_biome::{BiomeQuery, CubiomesQuery, builtin_biome_map};

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above be-corpus crate");
    let path = base.join("fixtures/biome-corpus-1.21.40.json");
    let corpus = Corpus::load(path.to_str().unwrap()).expect("biome corpus fixture loads");
    assert!(!corpus.biome_samples.is_empty());

    let map = builtin_biome_map();
    let mc = cubiomes_sys::mc_latest();
    let query_id = |seed: u64, x: i64, z: i64| -> Option<u16> {
        let q = CubiomesQuery::new(mc, seed);
        q.biome_id_at(x as i32, z as i32)
    };
    let resolve = |name: &str| -> Option<u16> { map.bedrock_id_for_name(name) };

    let rate = accuracy::biome_overall_rate(&corpus, query_id, resolve)
        .expect("biome corpus has comparable samples");

    // Pin the honest, currently-red figure (documented in be-biome lib.rs).
    // When this changes, the gate must be reviewed deliberately.
    assert!(
        (rate - 0.1818).abs() < 0.01,
        "biome agreement changed from pinned 18.2% to {:.1}% — review before flipping gate",
        rate * 100.0
    );
}
