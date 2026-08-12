//! Bedrock & Java numeric biome ID maps as data.

use std::collections::HashMap;

use serde::Deserialize;

/// Raw structure of `versions/biomes.json`.
#[derive(Debug, Deserialize)]
struct BiomeMapFile {
    java: HashMap<String, String>,
    bedrock: HashMap<String, String>,
    #[serde(default)]
    aliases: HashMap<String, Vec<u16>>,
}

/// Numeric biome ID maps (Bedrock & Java) plus legacy-name aliases.
#[derive(Debug, Clone)]
pub struct BiomeIdMap {
    /// Java numeric id -> Java biome name (what cubiomes emits).
    pub java: HashMap<u16, String>,
    /// Bedrock numeric id -> Bedrock biome name (what LevelDB Data3D emits).
    pub bedrock: HashMap<u16, String>,
    /// Legacy/structure-gate name -> numeric ids (e.g. "icePlains" -> [12]).
    pub aliases: HashMap<String, Vec<u16>>,
}

impl BiomeIdMap {
    pub fn from_json(json: &str) -> Result<BiomeIdMap, serde_json::Error> {
        let f: BiomeMapFile = serde_json::from_str(json)?;
        let parse = |m: HashMap<String, String>| {
            m.into_iter()
                .map(|(k, v)| (k.parse::<u16>().expect("biome id key is numeric"), v))
                .collect()
        };
        Ok(BiomeIdMap {
            java: parse(f.java),
            bedrock: parse(f.bedrock),
            aliases: f.aliases,
        })
    }

    /// Resolve a legacy/gate biome name to its numeric id(s). Returns `&[]` if the
    /// name is not known.
    pub fn resolve_alias(&self, name: &str) -> &[u16] {
        self.aliases.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Java biome name for a numeric id.
    pub fn java_name(&self, id: u16) -> Option<&str> {
        self.java.get(&id).map(String::as_str)
    }

    /// Bedrock biome name for a numeric id.
    pub fn bedrock_name(&self, id: u16) -> Option<&str> {
        self.bedrock.get(&id).map(String::as_str)
    }

    /// Bedrock numeric id for a Bedrock biome name (exact or via alias). Returns
    /// `None` if unknown. Used to resolve a `/locate biome` name to the id space
    /// shared with Java, so parity can be compared by id rather than by name.
    pub fn bedrock_id_for_name(&self, name: &str) -> Option<u16> {
        if let Some((id, _)) = self.bedrock.iter().find(|(_, n)| n.as_str() == name) {
            return Some(*id);
        }
        self.resolve_alias(name).first().copied()
    }
}

/// The built-in map, embedded from `versions/biomes.json` at compile time (the path
/// is relative to this source file: ../../../versions).
pub fn builtin_biome_map() -> BiomeIdMap {
    BiomeIdMap::from_json(include_str!("../../../versions/biomes.json"))
        .expect("embedded biomes.json must parse")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_parses_and_has_core_ids() {
        let m = builtin_biome_map();
        // Java names
        assert_eq!(m.java_name(1), Some("plains"));
        assert_eq!(m.java_name(2), Some("desert"));
        assert_eq!(m.java_name(183), Some("deep_dark"));
        // Bedrock names
        assert_eq!(m.bedrock_name(12), Some("ice_plains"));
        assert_eq!(m.bedrock_name(29), Some("roofed_forest"));
    }

    #[test]
    fn java_and_bedrock_share_id_set() {
        let m = builtin_biome_map();
        // Bedrock and Java share the legacy numeric id space for these biomes, so
        // every id must be present in both maps (a mismatch would silently mislabel
        // cubiomes vs LevelDB output).
        assert!(!m.java.is_empty());
        let java_keys: HashSet<u16> = m.java.keys().copied().collect();
        let bedrock_keys: HashSet<u16> = m.bedrock.keys().copied().collect();
        assert_eq!(java_keys, bedrock_keys, "java/bedrock id sets diverge");
    }

    #[test]
    fn every_version_table_gate_biome_resolves() {
        // Every biome name used in the 1.21.x structure version table must resolve to
        // at least one numeric id, otherwise its gate is a silent no-op.
        let m = builtin_biome_map();
        let v = be_struct::Version::builtin_1_21_40();
        for (key, s) in &v.structures {
            for biome in &s.biomes {
                assert!(
                    !m.resolve_alias(biome).is_empty(),
                    "structure {key} gate biome {biome:?} does not resolve to any id"
                );
            }
        }
    }

    #[test]
    fn alias_resolution() {
        let m = builtin_biome_map();
        assert_eq!(m.resolve_alias("swampland"), &[6]);
        assert_eq!(m.resolve_alias("icePlains"), &[12]);
        assert_eq!(m.resolve_alias("roofedForest"), &[29]);
        assert_eq!(m.resolve_alias("deep_frozen_ocean"), &[50]);
        assert!(m.resolve_alias("not_a_biome").is_empty());
    }
}
