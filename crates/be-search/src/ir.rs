//! Query intermediate representation (§3.3).
//!
//! Queries are **graphs, not flat lists**. Nodes are structure instances (variables to
//! bind) or biome-presence probes; edges are min–max distance constraints between two
//! anchors (origin or a bound variable). Both authoring surfaces — the route builder
//! and the text DSL — compile to this same `Query`, so the engine never knows which
//! produced it.
//!
//! The v1 constraint vocabulary (§3.3) is deliberately scoped:
//!
//! - relative distance between structures (min–max range)  ✅ in v1
//! - distance to origin (min–max range)                    ✅ in v1
//! - per-structure biome gate                              ✅ in v1
//! - biome present within radius                           ✅ in v1
//! - exclusion / counts / route geometry                   ⛔ deferred (space reserved)

use serde::{Deserialize, Serialize};

/// One endpoint of a distance edge. Either the world origin (0,0) or a bound variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Anchor {
    /// The world origin (0, 0) — where `/locate` searches from in Bedrock.
    Origin,
    /// A variable, by index into `Query.vars`.
    Var(usize),
}

impl std::fmt::Display for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Anchor::Origin => write!(f, "origin"),
            Anchor::Var(i) => write!(f, "v{i}"),
        }
    }
}

/// What a variable represents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarKind {
    /// A structure instance to bind, by its version-table key (e.g. "desert_pyramid").
    Structure(String),
    /// A standalone "biome present within radius" probe. The radius is the max of the
    /// incident edge; the biomes are what must be present somewhere in the window.
    BiomePresence { biomes: Vec<String> },
}

/// A variable (node) in the constraint graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Var {
    /// Human name shown in the UI / DSL (e.g. "v1", "t1").
    pub name: String,
    pub kind: VarKind,
    /// Per-structure biome gate: acceptable biomes the structure's anchor must land in.
    /// `None` = no gate (structure generates in any biome).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub biome_gate: Option<Vec<String>>,
}

/// A distance constraint between two anchors, inclusive on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub a: Anchor,
    pub b: Anchor,
    /// Minimum distance in blocks, inclusive.
    pub min: u32,
    /// Maximum distance in blocks, inclusive.
    pub max: u32,
}

impl Edge {
    pub fn contains(&self, d: u32) -> bool {
        d >= self.min && d <= self.max
    }
}

/// A fully-compiled query. Both authoring surfaces target this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub vars: Vec<Var>,
    pub edges: Vec<Edge>,
}

impl Query {
    /// Index of the variable named `name`, if present.
    pub fn var_index(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }

    /// All edges incident on a variable (either endpoint).
    pub fn edges_incident(&self, var: usize) -> impl Iterator<Item = &Edge> + '_ {
        let a = Anchor::Var(var);
        self.edges.iter().filter(move |e| e.a == a || e.b == a)
    }

    /// Resolve the other endpoint of an edge relative to `var`.
    pub fn other_anchor(&self, edge: &Edge, var: usize) -> Option<Anchor> {
        let a = Anchor::Var(var);
        if edge.a == a {
            Some(edge.b)
        } else if edge.b == a {
            Some(edge.a)
        } else {
            None
        }
    }

    /// Whether the graph is connected (every var reachable from var 0 via edges).
    /// Unused vars are pruned before this is meaningful; see [`crate::ir::PruneResult`].
    pub fn is_connected(&self) -> bool {
        if self.vars.is_empty() {
            return true;
        }
        let mut seen = vec![false; self.vars.len()];
        let mut stack = vec![0usize];
        seen[0] = true;
        while let Some(v) = stack.pop() {
            for e in self.edges_incident(v) {
                if let Some(Anchor::Var(o)) = self.other_anchor(e, v) {
                    if !seen[o] {
                        seen[o] = true;
                        stack.push(o);
                    }
                }
            }
        }
        seen.iter().all(|&s| s)
    }
}

/// Result of pruning unreachable variables/edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneResult {
    pub query: Query,
    pub dropped_vars: usize,
    pub dropped_edges: usize,
}

/// Remove variables with **no incident edge** (they can never be bound) and the edges
/// that reference dropped vars. A var connected only to origin is still meaningful (a
/// distance-to-origin constraint), so it is kept. Returns the trimmed graph plus a
/// count of what was removed.
pub fn prune_unconnected(query: &Query) -> PruneResult {
    if query.vars.is_empty() {
        return PruneResult {
            query: query.clone(),
            dropped_vars: 0,
            dropped_edges: 0,
        };
    }

    // A var is kept iff at least one edge references it.
    let mut has_edge = vec![false; query.vars.len()];
    for e in &query.edges {
        if let Anchor::Var(i) = e.a {
            has_edge[i] = true;
        }
        if let Anchor::Var(i) = e.b {
            has_edge[i] = true;
        }
    }

    // Remap kept var indices.
    let mut remap: Vec<Option<usize>> = vec![None; query.vars.len()];
    let mut kept_vars: Vec<Var> = Vec::new();
    let mut dropped_vars = 0usize;
    for (i, v) in query.vars.iter().enumerate() {
        if has_edge[i] {
            remap[i] = Some(kept_vars.len());
            kept_vars.push(v.clone());
        } else {
            dropped_vars += 1;
        }
    }

    let mut kept_edges = Vec::new();
    let mut dropped_edges = 0usize;
    for e in &query.edges {
        let map = |a: Anchor| -> Option<Anchor> {
            match a {
                Anchor::Origin => Some(Anchor::Origin),
                Anchor::Var(i) => remap[i].map(Anchor::Var),
            }
        };
        match (map(e.a), map(e.b)) {
            (Some(a), Some(b)) => kept_edges.push(Edge {
                a,
                b,
                min: e.min,
                max: e.max,
            }),
            _ => dropped_edges += 1,
        }
    }

    PruneResult {
        query: Query {
            vars: kept_vars,
            edges: kept_edges,
        },
        dropped_vars,
        dropped_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_query() -> Query {
        Query {
            vars: vec![
                Var {
                    name: "v1".into(),
                    kind: VarKind::Structure("village".into()),
                    biome_gate: None,
                },
                Var {
                    name: "t1".into(),
                    kind: VarKind::Structure("desert_pyramid".into()),
                    biome_gate: Some(vec!["desert".into()]),
                },
                Var {
                    name: "m1".into(),
                    kind: VarKind::Structure("ocean_monument".into()),
                    biome_gate: None,
                },
            ],
            edges: vec![
                Edge {
                    a: Anchor::Origin,
                    b: Anchor::Var(0),
                    min: 0,
                    max: 800,
                },
                Edge {
                    a: Anchor::Var(0),
                    b: Anchor::Var(1),
                    min: 600,
                    max: 1200,
                },
                Edge {
                    a: Anchor::Var(1),
                    b: Anchor::Var(2),
                    min: 0,
                    max: 1500,
                },
            ],
        }
    }

    #[test]
    fn edges_incident_and_other_anchor() {
        let q = sample_query();
        let e = q.edges[1];
        assert_eq!(q.other_anchor(&e, 0), Some(Anchor::Var(1)));
        assert_eq!(q.other_anchor(&e, 1), Some(Anchor::Var(0)));
        assert_eq!(q.other_anchor(&e, 2), None);
    }

    #[test]
    fn query_is_connected() {
        assert!(sample_query().is_connected());
    }

    #[test]
    fn edge_range_is_inclusive() {
        let e = Edge {
            a: Anchor::Origin,
            b: Anchor::Var(0),
            min: 600,
            max: 1200,
        };
        assert!(!e.contains(599));
        assert!(e.contains(600));
        assert!(e.contains(1200));
        assert!(!e.contains(1201));
    }

    #[test]
    fn prune_drops_disconnected_var() {
        let mut q = sample_query();
        // Add an island var with no edges.
        q.vars.push(Var {
            name: "x1".into(),
            kind: VarKind::Structure("woodland_mansion".into()),
            biome_gate: None,
        });
        let pruned = prune_unconnected(&q);
        assert_eq!(pruned.dropped_vars, 1);
        assert_eq!(pruned.query.vars.len(), 3);
        // Remapped indices are contiguous.
        assert!(!pruned.query.vars.iter().any(|v| v.name == "x1"));
    }

    #[test]
    fn prune_remaps_edges_correctly() {
        let mut q = sample_query();
        q.vars.push(Var {
            name: "x1".into(),
            kind: VarKind::Structure("woodland_mansion".into()),
            biome_gate: None,
        });
        // Connect x1 (index 3) to origin so it survives, then check remapping.
        q.edges.push(Edge {
            a: Anchor::Origin,
            b: Anchor::Var(3),
            min: 0,
            max: 5000,
        });
        let pruned = prune_unconnected(&q);
        assert_eq!(pruned.dropped_vars, 0);
        assert_eq!(pruned.query.vars.len(), 4);
        // The edge to x1 should still point at the correct (same-index) var.
        assert!(pruned.query.edges.iter().any(|e| e.a == Anchor::Var(3) || e.b == Anchor::Var(3)));
    }
}
