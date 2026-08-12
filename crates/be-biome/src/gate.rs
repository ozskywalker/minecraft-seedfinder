//! Structure biome-gate evaluation (PLAN §2.6).
//!
//! A structure only generates if its biome gate is satisfied (e.g. Desert Pyramid ⇒
//! `{desert, desert_hills, desert_lakes}`). This module turns a gate's legacy names
//! into a set of numeric ids and tests a queried id against it. It is also the basis
//! for the §2.6 cheap-biome-probe: "a swamp hut generated here ⇒ this is swamp".

use std::collections::HashSet;

use be_struct::Version;

use crate::map::BiomeIdMap;

/// A structure's acceptable biome ids.
#[derive(Debug, Clone, Default)]
pub struct BiomeGate {
    ids: HashSet<u16>,
    /// True when the structure has no biome constraint (generates in any biome).
    unconstrained: bool,
}

impl BiomeGate {
    /// Build a gate from a list of legacy biome names, resolving each through the
    /// map. An empty name list means "unconstrained" (no biome gate).
    pub fn from_names(map: &BiomeIdMap, names: &[String]) -> BiomeGate {
        let mut ids = HashSet::new();
        for name in names {
            for id in map.resolve_alias(name) {
                ids.insert(*id);
            }
        }
        BiomeGate {
            ids,
            unconstrained: names.is_empty(),
        }
    }

    /// Whether the given biome id satisfies this gate. An unconstrained gate accepts
    /// everything.
    pub fn passes(&self, id: u16) -> bool {
        self.unconstrained || self.ids.contains(&id)
    }

    pub fn is_unconstrained(&self) -> bool {
        self.unconstrained
    }

    pub fn ids(&self) -> &HashSet<u16> {
        &self.ids
    }
}

/// Build the biome gate for a structure from a version table.
pub fn structure_gate(map: &BiomeIdMap, version: &Version, structure: &str) -> Option<BiomeGate> {
    let s = version.structures.get(structure)?;
    Some(BiomeGate::from_names(map, &s.biomes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::builtin_biome_map;

    fn map() -> BiomeIdMap {
        builtin_biome_map()
    }

    #[test]
    fn desert_gate_accepts_desert_only() {
        let v = Version::builtin_1_21_40();
        let gate = structure_gate(&map(), &v, "desert_pyramid").unwrap();
        assert!(gate.passes(2)); // desert
        assert!(gate.passes(17)); // desert_hills
        assert!(!gate.passes(1)); // plains
        assert!(!gate.passes(21)); // jungle
    }

    #[test]
    fn swamp_gate_resolves_swampland_alias() {
        let v = Version::builtin_1_21_40();
        let gate = structure_gate(&map(), &v, "swamp_hut").unwrap();
        // The table lists "swamp" and "swampland", both alias to id 6.
        assert!(gate.passes(6));
        assert!(!gate.passes(2));
    }

    #[test]
    fn mansion_gate_roofed_forest() {
        let v = Version::builtin_1_21_40();
        let gate = structure_gate(&map(), &v, "woodland_mansion").unwrap();
        assert!(gate.passes(29)); // roofed_forest / dark_forest
        assert!(!gate.passes(1));
    }

    #[test]
    fn empty_gate_is_unconstrained() {
        let gate = BiomeGate::from_names(&map(), &[]);
        assert!(gate.is_unconstrained());
        assert!(gate.passes(0));
        assert!(gate.passes(183));
    }
}
