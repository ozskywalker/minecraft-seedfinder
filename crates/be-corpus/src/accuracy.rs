//! Accuracy computation and reporting.

use be_struct::{structure_block_pos, Version};

use crate::corpus::{BlockPos, Corpus};

/// Per-(version, structure) accuracy summary.
#[derive(Debug, Clone)]
pub struct StructureAccuracy {
    pub version: String,
    pub structure: String,
    pub n: usize,
    /// Number exactly matching the prediction.
    pub exact: usize,
    /// Number within `tolerance` blocks of the prediction.
    pub within_tolerance: usize,
    /// `within_tolerance / n` (0.0 when n == 0).
    pub rate: f64,
    pub mean_dist: f64,
    pub max_dist: f64,
    /// Mean signed offset (observed - predicted) in x and z. A large systematic
    /// offset here is the [UNCONFIRMED] anchor-vs-centre problem (PLAN §4).
    pub mean_offset_x: f64,
    pub mean_offset_z: f64,
    pub tolerance: u64,
}

/// A full accuracy report across all (version, structure) groups in a corpus.
#[derive(Debug, Clone)]
pub struct AccuracyReport {
    pub groups: Vec<StructureAccuracy>,
    pub tolerance: u64,
}

impl AccuracyReport {
    /// Render an aligned, human-readable table.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "accuracy report (within {} blocks)\n",
            self.tolerance
        ));
        out.push_str(&format!(
            "{:<10} {:<20} {:>4} {:>5} {:>5} {:>7} {:>9} {:>9} {:>10} {:>10}\n",
            "version",
            "structure",
            "n",
            "exact",
            "<=tol",
            "rate%",
            "mean",
            "max",
            "off.dx",
            "off.dz"
        ));
        for g in &self.groups {
            out.push_str(&format!(
                "{:<10} {:<20} {:>4} {:>5} {:>5} {:>7.1} {:>9.1} {:>9.1} {:>10.1} {:>10.1}\n",
                g.version,
                g.structure,
                g.n,
                g.exact,
                g.within_tolerance,
                g.rate * 100.0,
                g.mean_dist,
                g.max_dist,
                g.mean_offset_x,
                g.mean_offset_z,
            ));
        }
        out
    }
}

/// Compute an accuracy report for a corpus against a version table.
///
/// For each sample we back out the region the game chose from the observed position
/// (`region_of_block`) and recompute the predicted position for that region with
/// `be-struct`. Samples whose structure is absent from the version table are skipped
/// (counted nowhere, since we cannot predict them).
pub fn compute_accuracy(corpus: &Corpus, version: &Version, tolerance: u64) -> AccuracyReport {
    // Group by (version, structure).
    let mut groups: Vec<StructureAccuracy> = Vec::new();

    for sample in &corpus.samples {
        let struct_params = match version.structures.get(&sample.structure) {
            Some(s) => s,
            None => continue, // structure not modelled for this version
        };

        let reg_x = be_struct::region_of_block(sample.observed.x, struct_params.spacing);
        let reg_z = be_struct::region_of_block(sample.observed.z, struct_params.spacing);
        let predicted = structure_block_pos(
            sample.seed,
            reg_x,
            reg_z,
            struct_params.salt,
            struct_params.spacing,
            struct_params.chunk_range,
            struct_params.distribution(),
        );
        let pred = BlockPos::new(predicted.0, predicted.1);
        let dist = pred.distance_to(&sample.observed);
        let dx = (sample.observed.x - pred.x) as f64;
        let dz = (sample.observed.z - pred.z) as f64;

        let g = groups
            .iter_mut()
            .find(|g| g.version == sample.version && g.structure == sample.structure);
        match g {
            Some(g) => {
                g.n += 1;
                if dist == 0.0 {
                    g.exact += 1;
                }
                if dist <= tolerance as f64 {
                    g.within_tolerance += 1;
                }
                g.mean_dist += dist;
                g.max_dist = g.max_dist.max(dist);
                g.mean_offset_x += dx;
                g.mean_offset_z += dz;
            }
            None => {
                let mut g = StructureAccuracy {
                    version: sample.version.clone(),
                    structure: sample.structure.clone(),
                    n: 1,
                    exact: if dist == 0.0 { 1 } else { 0 },
                    within_tolerance: if dist <= tolerance as f64 { 1 } else { 0 },
                    rate: 0.0,
                    mean_dist: dist,
                    max_dist: dist,
                    mean_offset_x: dx,
                    mean_offset_z: dz,
                    tolerance,
                };
                g.rate = g.within_tolerance as f64 / g.n as f64;
                groups.push(g);
            }
        }
    }

    // Finalize rates/means.
    for g in &mut groups {
        g.rate = g.within_tolerance as f64 / g.n as f64;
        g.mean_dist /= g.n as f64;
        g.mean_offset_x /= g.n as f64;
        g.mean_offset_z /= g.n as f64;
    }

    groups.sort_by(|a, b| {
        a.version
            .cmp(&b.version)
            .then(a.structure.cmp(&b.structure))
    });

    AccuracyReport { groups, tolerance }
}

/// Convenience: the overall within-tolerance rate across all samples (used as the
/// CI gate number). Returns `None` if there are no comparable samples.
pub fn overall_rate(corpus: &Corpus, version: &Version, tolerance: u64) -> Option<f64> {
    let report = compute_accuracy(corpus, version, tolerance);
    let total: usize = report.groups.iter().map(|g| g.within_tolerance).sum();
    let n: usize = report.groups.iter().map(|g| g.n).sum();
    if n == 0 {
        None
    } else {
        Some(total as f64 / n as f64)
    }
}

/// A per-biome agreement summary.
#[derive(Debug, Clone)]
pub struct BiomeAgreement {
    pub biome: String,
    pub n: usize,
    pub matches: usize,
    pub rate: f64,
}

/// Per-biome agreement report across a corpus of [`BiomeSample`]s.
#[derive(Debug, Clone)]
pub struct BiomeAgreementReport {
    pub groups: Vec<BiomeAgreement>,
}

impl BiomeAgreementReport {
    pub fn render(&self) -> String {
        let mut out = String::from("biome agreement report\n");
        out.push_str(&format!(
            "{:<24} {:>4} {:>6} {:>8}\n",
            "biome", "n", "match", "rate%"
        ));
        for g in &self.groups {
            out.push_str(&format!(
                "{:<24} {:>4} {:>6} {:>7.1}\n",
                g.biome,
                g.n,
                g.matches,
                g.rate * 100.0
            ));
        }
        out
    }
}

/// Compare every biome sample against a predicate that reports the Java biome id at
/// the observed block coordinate. A sample matches when the Java id (what cubiomes
/// predicts) equals the Bedrock id resolved from the observed biome name.
///
/// `query_id` maps `(seed, x, z) -> Option<u16>` (the cubiomes/Java biome id at that
/// position). `resolve_bedrock_id` maps a biome name (as `/locate biome` reported it)
/// to the shared numeric id space. Bedrock↔Java parity is validated by checking
/// whether the cubiomes model agrees with the real `/locate biome` at that coordinate.
pub fn compute_biome_agreement<F, R>(
    corpus: &Corpus,
    query_id: F,
    resolve_bedrock_id: R,
) -> BiomeAgreementReport
where
    F: Fn(u64, i64, i64) -> Option<u16>,
    R: Fn(&str) -> Option<u16>,
{
    let mut map: Vec<BiomeAgreement> = Vec::new();
    for s in &corpus.biome_samples {
        let target = resolve_bedrock_id(&s.biome);
        let matched = match target {
            Some(tid) => query_id(s.seed, s.observed.x, s.observed.z) == Some(tid),
            None => false, // unknown biome name: not counted as a match
        };
        let g = map.iter_mut().find(|g| g.biome == s.biome);
        match g {
            Some(g) => {
                g.n += 1;
                if matched {
                    g.matches += 1;
                }
            }
            None => {
                map.push(BiomeAgreement {
                    biome: s.biome.clone(),
                    n: 1,
                    matches: if matched { 1 } else { 0 },
                    rate: 0.0,
                });
            }
        }
    }
    for g in &mut map {
        g.rate = g.matches as f64 / g.n as f64;
    }
    map.sort_by(|a, b| a.biome.cmp(&b.biome));
    BiomeAgreementReport { groups: map }
}

/// Overall biome agreement rate across all biome samples (the §8 parity gate number).
pub fn biome_overall_rate<F, R>(corpus: &Corpus, query_id: F, resolve_bedrock_id: R) -> Option<f64>
where
    F: Fn(u64, i64, i64) -> Option<u16>,
    R: Fn(&str) -> Option<u16>,
{
    let report = compute_biome_agreement(corpus, query_id, resolve_bedrock_id);
    let total: usize = report.groups.iter().map(|g| g.matches).sum();
    let n: usize = report.groups.iter().map(|g| g.n).sum();
    if n == 0 {
        None
    } else {
        Some(total as f64 / n as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{BlockPos, Sample};
    use be_struct::Distribution;

    fn version() -> Version {
        Version::builtin_1_21_40()
    }

    /// A sample whose observed position exactly matches what be-struct predicts for
    /// that region must score exact.
    #[test]
    fn exact_match_scores_one() {
        let v = version();
        let s = &v.structures["village"];
        // Choose a seed/region, compute the "game" position as our own prediction
        // (self-consistency): the accuracy layer should then report exact.
        let seed = 1234u64;
        let (bx, bz) = structure_block_pos(
            seed,
            2,
            -1,
            s.salt,
            s.spacing,
            s.chunk_range,
            Distribution::Triangular,
        );
        let mut c = Corpus::new();
        c.push(Sample {
            version: "1.21.40".into(),
            seed,
            structure: "village".into(),
            observed: BlockPos::new(bx, bz),
        });
        let report = compute_accuracy(&c, &v, 16);
        assert_eq!(report.groups.len(), 1);
        let g = &report.groups[0];
        assert_eq!(g.n, 1);
        assert_eq!(g.exact, 1);
        assert_eq!(g.within_tolerance, 1);
        assert_eq!(g.rate, 1.0);
    }

    /// A sample offset by a known amount reports that offset and falls outside a
    /// tighter tolerance.
    #[test]
    fn offset_affects_tolerance_and_means() {
        let v = version();
        let s = &v.structures["desert_pyramid"]; // linear
        let seed = 999u64;
        let (bx, bz) = structure_block_pos(
            seed,
            0,
            0,
            s.salt,
            s.spacing,
            s.chunk_range,
            Distribution::Linear,
        );
        // Observed shifted +8 in x (a plausible anchor-vs-centre offset).
        let mut c = Corpus::new();
        c.push(Sample {
            version: "1.21.40".into(),
            seed,
            structure: "desert_pyramid".into(),
            observed: BlockPos::new(bx + 8, bz),
        });
        let report = compute_accuracy(&c, &v, 4);
        let g = &report.groups[0];
        assert_eq!(g.exact, 0);
        assert_eq!(g.within_tolerance, 0);
        assert!((g.mean_offset_x - 8.0).abs() < 1e-9);
        assert!((g.mean_dist - 8.0).abs() < 1e-9);

        // A tolerance >= 8 absorbs it.
        let report8 = compute_accuracy(&c, &v, 8);
        assert_eq!(report8.groups[0].within_tolerance, 1);
    }

    /// Unknown structures are skipped (no prediction possible).
    #[test]
    fn unknown_structure_is_skipped() {
        let v = version();
        let mut c = Corpus::new();
        c.push(Sample {
            version: "1.21.40".into(),
            seed: 1,
            structure: "not_a_real_structure".into(),
            observed: BlockPos::new(0, 0),
        });
        let report = compute_accuracy(&c, &v, 16);
        assert!(report.groups.is_empty());
        assert_eq!(overall_rate(&c, &v, 16), None);
    }

    #[test]
    fn overall_rate_across_groups() {
        let v = version();
        let s = &v.structures["village"];
        let seed = 555u64;
        let (bx, bz) = structure_block_pos(
            seed,
            1,
            1,
            s.salt,
            s.spacing,
            s.chunk_range,
            Distribution::Triangular,
        );
        let mut c = Corpus::new();
        // one exact, one off by 40 (outside tol 16)
        c.push(Sample {
            version: "1.21.40".into(),
            seed,
            structure: "village".into(),
            observed: BlockPos::new(bx, bz),
        });
        c.push(Sample {
            version: "1.21.40".into(),
            seed,
            structure: "village".into(),
            observed: BlockPos::new(bx + 40, bz),
        });
        assert_eq!(overall_rate(&c, &v, 16), Some(0.5));
    }

    #[test]
    fn report_renders_table() {
        let v = version();
        let s = &v.structures["village"];
        let (bx, bz) = structure_block_pos(
            5,
            0,
            0,
            s.salt,
            s.spacing,
            s.chunk_range,
            Distribution::Triangular,
        );
        let mut c = Corpus::new();
        c.push(Sample {
            version: "1.21.40".into(),
            seed: 5,
            structure: "village".into(),
            observed: BlockPos::new(bx, bz),
        });
        let report = compute_accuracy(&c, &v, 16);
        let text = report.render();
        assert!(text.contains("village"));
        assert!(text.contains("accuracy report"));
    }

    // ---- biome agreement ----

    fn biome_corpus() -> Corpus {
        let mut c = Corpus::new();
        c.push_biome(crate::corpus::BiomeSample {
            version: "1.21.40".into(),
            seed: 1,
            biome: "plains".into(),
            observed: BlockPos::new(100, 200),
        });
        c.push_biome(crate::corpus::BiomeSample {
            version: "1.21.40".into(),
            seed: 1,
            biome: "desert".into(),
            observed: BlockPos::new(-50, 30),
        });
        c
    }

    #[test]
    fn biome_agreement_counts_matches() {
        // query_id returns plains (1) everywhere; desert (2) resolves to 2 but query
        // says 1, so desert mismatches.
        let query = |_s: u64, _x: i64, _z: i64| -> Option<u16> { Some(1) };
        let resolve = |name: &str| -> Option<u16> {
            match name {
                "plains" => Some(1),
                "desert" => Some(2),
                _ => None,
            }
        };
        let report = compute_biome_agreement(&biome_corpus(), query, resolve);
        assert_eq!(report.groups.len(), 2);
        let plains = report.groups.iter().find(|g| g.biome == "plains").unwrap();
        assert_eq!(plains.matches, 1);
        assert_eq!(plains.n, 1);
        let desert = report.groups.iter().find(|g| g.biome == "desert").unwrap();
        assert_eq!(desert.matches, 0);
        assert_eq!(desert.rate, 0.0);
        assert_eq!(
            biome_overall_rate(&biome_corpus(), query, resolve),
            Some(0.5)
        );
    }

    #[test]
    fn biome_agreement_unknown_biome_counts_as_mismatch() {
        let mut c = Corpus::new();
        c.push_biome(crate::corpus::BiomeSample {
            version: "1.21.40".into(),
            seed: 1,
            biome: "not_a_biome".into(),
            observed: BlockPos::new(0, 0),
        });
        // query says plains (1); the unknown name resolves to None -> not a match.
        let query = |_s: u64, _x: i64, _z: i64| -> Option<u16> { Some(1) };
        let resolve = |_n: &str| -> Option<u16> { None };
        let report = compute_biome_agreement(&c, query, resolve);
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].matches, 0);
    }

    #[test]
    fn biome_agreement_empty_corpus_gives_none() {
        let c = Corpus::new();
        let query = |_s: u64, _x: i64, _z: i64| -> Option<u16> { Some(1) };
        let resolve = |_n: &str| -> Option<u16> { Some(1) };
        assert_eq!(biome_overall_rate(&c, query, resolve), None);
    }
}
