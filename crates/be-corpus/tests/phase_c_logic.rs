//! Offline regression gate for Phase C decision logic (PLAN §7 "Phase C").
//!
//! The live Phase C step (`be-corpus verify-seed[s]`) recreates a real world per seed
//! and `/locate`s each structure. Its *decision logic* — [`be_corpus::compare`],
//! [`be_corpus::predict_for_region`] and [`be_corpus::Verdict`] — is pure and must be
//! pinned offline so a silent change to the PASS/FAIL/SKIP semantics (e.g. a tolerance
//! or anchor-vs-centre regression) fails loudly in CI without any server.
//!
//! This complements the existing unit tests inside `verify.rs`; it formalizes them as
//! an integration gate and adds the structures the live corpus now covers (including
//! `woodland_mansion`).

use be_corpus::{compare, predict_for_region, BlockPos, Verdict, Version, ANCHOR_STRUCTURES};
use be_struct::structure_block_pos;
use be_verify::LocateResult;

fn version() -> Version {
    Version::builtin_1_21_40()
}

fn pos(x: i64, z: i64) -> BlockPos {
    BlockPos::new(x, z)
}

/// The full PASS/FAIL/SKIP decision matrix for `compare`, pinned exactly.
#[test]
fn compare_decision_matrix_is_pinned() {
    let tol = 16;

    // PASS: within tolerance.
    assert!(matches!(
        compare(
            Some(pos(100, 200)),
            Some(LocateResult::Found {
                x: 108,
                z: 200,
                y: None
            }),
            tol
        ),
        Verdict::Pass
    ));

    // FAIL: beyond tolerance.
    assert!(matches!(
        compare(
            Some(pos(100, 200)),
            Some(LocateResult::Found {
                x: 1000,
                z: 2000,
                y: None
            }),
            tol
        ),
        Verdict::Fail { .. }
    ));

    // SKIP: structure not modelled (cannot predict).
    assert!(matches!(
        compare(None, Some(LocateResult::NotFound), tol),
        Verdict::Skip { .. }
    ));

    // SKIP: structure absent near origin (no region to back out). Not a FAIL — a
    // sparse structure (e.g. woodland_mansion) legitimately SKIPs.
    assert!(matches!(
        compare(Some(pos(0, 0)), Some(LocateResult::NotFound), tol),
        Verdict::Skip { .. }
    ));

    // SKIP: inconclusive observation (no parseable response / no response at all).
    assert!(matches!(
        compare(Some(pos(0, 0)), None, tol),
        Verdict::Skip { .. }
    ));
    assert!(matches!(
        compare(
            Some(pos(0, 0)),
            Some(LocateResult::Unparseable("x".into())),
            tol
        ),
        Verdict::Skip { .. }
    ));
}

/// Tolerance edge: distance exactly at the tolerance boundary passes.
#[test]
fn compare_tolerance_boundary() {
    // 16 blocks exactly → PASS (<= tolerance).
    assert!(matches!(
        compare(
            Some(pos(0, 0)),
            Some(LocateResult::Found {
                x: 16,
                z: 0,
                y: None
            }),
            16
        ),
        Verdict::Pass
    ));
    // 17 blocks → FAIL.
    assert!(matches!(
        compare(
            Some(pos(0, 0)),
            Some(LocateResult::Found {
                x: 17,
                z: 0,
                y: None
            }),
            16
        ),
        Verdict::Fail { .. }
    ));
}

/// Every anchor structure (including the sparse `woodland_mansion`) must be modelled
/// and predict a region-consistent position for a fabricated observation.
#[test]
fn every_anchor_structure_is_modelled_and_predicts() {
    let v = version();
    assert!(
        ANCHOR_STRUCTURES.contains(&"woodland_mansion"),
        "woodland_mansion must be in the default Phase C anchor set"
    );
    for id in ANCHOR_STRUCTURES {
        let sp = &v.structures[id];
        let (bx, bz) = structure_block_pos(
            12345678,
            1,
            -2,
            sp.salt,
            sp.spacing,
            sp.chunk_range,
            sp.distribution(),
        );
        let observed = BlockPos::new(bx, bz);
        let predicted = predict_for_region(&v, id, 12345678, observed)
            .unwrap_or_else(|| panic!("{id} not modelled"));
        // Self-consistency: prediction of the region the observation lies in must
        // reproduce the observation (the compare layer then reports PASS).
        assert_eq!(
            predicted, observed,
            "{id} region-backed-out must reproduce placement"
        );
        assert_eq!(
            compare(
                predict_for_region(&v, id, 12345678, observed),
                Some(LocateResult::Found {
                    x: observed.x,
                    z: observed.z,
                    y: None
                }),
                16
            ),
            Verdict::Pass,
            "{id} self-consistent observation must PASS"
        );
    }
}

/// The shared-salt scattered set must predict identical placement across all four ids
/// (they share salt/spacing/chunk_range/distribution), so a single `/locate structure
/// temple` observation validates all four at once.
#[test]
fn scattered_shared_placement_is_identical() {
    use be_corpus::scattered::predicted_for_region;
    let v = version();
    let preds = predicted_for_region(&v, 999, BlockPos::new(1000, -2000));
    assert_eq!(preds.len(), 4, "all four scattered ids modelled");
    let first = preds[0].1;
    for (id, p) in &preds {
        assert_eq!(*p, first, "id {id} diverged from shared placement");
    }
}

/// `predict_for_region` returns `None` for a structure absent from the version table
/// (the compare layer then SKIPs).
#[test]
fn predict_for_region_unknown_structure_is_none() {
    let v = version();
    assert!(predict_for_region(&v, "not_a_real_structure", 1, pos(0, 0)).is_none());
}
