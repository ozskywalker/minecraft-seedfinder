//! Corpus fixture format and storage.

use serde::{Deserialize, Serialize};

/// A 2D block position (horizontal only; structure geometry ignores Y).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockPos {
    pub x: i64,
    pub z: i64,
}

impl BlockPos {
    pub fn new(x: i64, z: i64) -> Self {
        BlockPos { x, z }
    }

    /// Euclidean horizontal distance to another position.
    pub fn distance_to(&self, other: &BlockPos) -> f64 {
        let dx = (self.x - other.x) as f64;
        let dz = (self.z - other.z) as f64;
        (dx * dx + dz * dz).sqrt()
    }
}

/// A single ground-truth observation: the real game placed `structure` at `observed`
/// for `seed` on `version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub version: String,
    pub seed: u64,
    pub structure: String,
    pub observed: BlockPos,
}

/// A ground-truth biome observation: the real game reported the nearest instance of
/// `biome` at `observed` (from `/locate biome`). Used for the §8 Bedrock↔Java biome
/// parity validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiomeSample {
    pub version: String,
    pub seed: u64,
    /// Biome name (without the `minecraft:` namespace).
    pub biome: String,
    pub observed: BlockPos,
}

/// An ordered collection of samples.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Corpus {
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub biome_samples: Vec<BiomeSample>,
}

impl Corpus {
    pub fn new() -> Self {
        Corpus::default()
    }

    pub fn push(&mut self, sample: Sample) {
        self.samples.push(sample);
    }

    pub fn push_biome(&mut self, sample: BiomeSample) {
        self.biome_samples.push(sample);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Serialize to JSON text (pretty-printed).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON text.
    pub fn from_json(json: &str) -> serde_json::Result<Corpus> {
        serde_json::from_str(json)
    }

    /// Write to a file.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let json = self.to_json().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        std::fs::write(path, json)
    }

    /// Load from a file.
    pub fn load(path: &str) -> std::io::Result<Corpus> {
        let json = std::fs::read_to_string(path)?;
        Corpus::from_json(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            version: "1.21.40".into(),
            seed: 1234,
            structure: "village".into(),
            observed: BlockPos::new(1234, -5678),
        }
    }

    #[test]
    fn round_trip_json() {
        let mut c = Corpus::new();
        c.push(sample());
        let json = c.to_json().unwrap();
        let back = Corpus::from_json(&json).unwrap();
        assert_eq!(back.samples.len(), 1);
        assert_eq!(back.samples[0].structure, "village");
        assert_eq!(back.samples[0].observed, BlockPos::new(1234, -5678));
    }

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join("be-corpus-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corpus.json");
        let path_str = path.to_str().unwrap();

        let mut c = Corpus::new();
        c.push(sample());
        c.save(path_str).unwrap();
        let back = Corpus::load(path_str).unwrap();
        assert_eq!(back, c);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn distance() {
        assert_eq!(BlockPos::new(0, 0).distance_to(&BlockPos::new(3, 4)), 5.0);
        assert_eq!(BlockPos::new(0, 0).distance_to(&BlockPos::new(0, 0)), 0.0);
    }
}
