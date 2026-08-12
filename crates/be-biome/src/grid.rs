//! 2D biome grids and predicted-vs-observed agreement.
//!
//! This is the validation plumbing behind PLAN §4's dense biome truth: a *predicted*
//! grid (from `BiomeQuery`/cubiomes) compared against an *observed* grid (read from
//! a Bedrock LevelDB `Data3D` record). We cannot read real LevelDB worlds here, so
//! [`compute_agreement`] is tested with synthetic grids; wiring real `Data3D`
//! decoding is deferred until that data exists.

/// A 2D grid of biome numeric ids, row-major (`ids[z*width + x]`).
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeGrid {
    pub width: usize,
    pub height: usize,
    pub ids: Vec<u16>,
}

impl BiomeGrid {
    pub fn new(width: usize, height: usize, ids: Vec<u16>) -> Option<BiomeGrid> {
        if ids.len() != width * height {
            return None;
        }
        Some(BiomeGrid { width, height, ids })
    }

    pub fn get(&self, x: usize, z: usize) -> u16 {
        self.ids[z * self.width + x]
    }
}

/// Agreement between a predicted and an observed biome grid.
#[derive(Debug, Clone)]
pub struct AgreementReport {
    pub cells: usize,
    /// Cells where predicted id == observed id.
    pub matches: usize,
    pub rate: f64,
}

impl AgreementReport {
    pub fn render(&self) -> String {
        format!(
            "biome agreement: {}/{} cells ({:.1}%)",
            self.matches,
            self.cells,
            self.rate * 100.0
        )
    }
}

/// Compare two same-sized grids cell-by-cell, reporting the match rate.
///
/// Returns `None` if the grids differ in dimensions.
pub fn compute_agreement(predicted: &BiomeGrid, observed: &BiomeGrid) -> Option<AgreementReport> {
    if predicted.width != observed.width || predicted.height != observed.height {
        return None;
    }
    let cells = predicted.ids.len();
    let matches = predicted
        .ids
        .iter()
        .zip(observed.ids.iter())
        .filter(|(a, b)| a == b)
        .count();
    Some(AgreementReport {
        cells,
        matches,
        rate: matches as f64 / cells as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_grids_agree_fully() {
        let ids = vec![1u16, 2, 3, 4];
        let a = BiomeGrid::new(2, 2, ids.clone()).unwrap();
        let b = BiomeGrid::new(2, 2, ids).unwrap();
        let r = compute_agreement(&a, &b).unwrap();
        assert_eq!(r.cells, 4);
        assert_eq!(r.matches, 4);
        assert_eq!(r.rate, 1.0);
    }

    #[test]
    fn partial_agreement() {
        // predicted: [1,1,2,2] ; observed: [1,1,2,3] -> 3/4 match
        let p = BiomeGrid::new(2, 2, vec![1, 1, 2, 2]).unwrap();
        let o = BiomeGrid::new(2, 2, vec![1, 1, 2, 3]).unwrap();
        let r = compute_agreement(&p, &o).unwrap();
        assert_eq!(r.matches, 3);
        assert!((r.rate - 0.75).abs() < 1e-9);
    }

    #[test]
    fn dimension_mismatch_returns_none() {
        let p = BiomeGrid::new(2, 2, vec![1, 2, 3, 4]).unwrap();
        let o = BiomeGrid::new(4, 1, vec![1, 2, 3, 4]).unwrap();
        assert!(compute_agreement(&p, &o).is_none());
    }

    #[test]
    fn grid_constructor_validates_size() {
        assert!(BiomeGrid::new(2, 2, vec![1, 2, 3]).is_none());
    }

    #[test]
    fn render_reports_rate() {
        let p = BiomeGrid::new(1, 2, vec![1, 2]).unwrap();
        let o = BiomeGrid::new(1, 2, vec![1, 9]).unwrap();
        let r = compute_agreement(&p, &o).unwrap();
        assert!(r.render().contains("1/2 cells (50.0%)"));
    }
}
