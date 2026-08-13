//! Text DSL — the escape hatch for queries the linear route builder can't express.
//!
//! It is version-controllable, diffable and unit-testable, which directly serves the
//! not-brittle goal: every example query becomes a regression test, and the DSL corpus
//! doubles as the parser suite (§5 "DSL"). It round-trips to and from [`Query`], so the
//! route builder can start visually and graduate to text without losing work.
//!
//! ## Grammar (one statement per line, `#` comments, blank lines ignored)
//!
//! The format follows PLAN §3.3 verbatim:
//!
//! ```text
//! <structure> <var> @ <anchor> <range> [; biome=<b1>,<b2>]
//! ```
//!
//! - `<structure>` — a version-table structure key, or the keyword `biome` for a
//!   biome-presence probe.
//! - `<var>` — a human variable name (e.g. `v1`, `t1`).
//! - `<anchor>` — `origin` or a previously-declared variable name.
//! - `<range>` — `<= N`, `>= N`, or `in A..B` (inclusive blocks).
//! - `biome=` — optional per-structure biome gate (comma-separated names).
//!
//! ## Example (from PLAN §3.3)
//!
//! ```text
//! village        v1  @origin <= 800
//! desert_pyramid t1  @v1 in 600..1200, biome=desert
//! ocean_monument m1  @t1 <= 1500
//! woodland_mansion x1 @origin >= 3000
//! ```

use std::collections::HashMap;

use crate::ir::{Anchor, Edge, Query, Var, VarKind};

/// Parse a DSL string into a [`Query`].
///
/// # Errors
/// Returns the offending line plus a message on syntax or semantic error (unknown
/// anchor, bad range, malformed line).
pub fn parse(input: &str) -> Result<Query, DslError> {
    let mut vars: Vec<Var> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut declared: HashMap<String, usize> = HashMap::new();

    for (lineno, raw) in input.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let (stmt, var_idx) = parse_statement(line, lineno + 1, &declared)?;
        declared.insert(stmt.var_name.clone(), var_idx);
        vars.push(Var {
            name: stmt.var_name,
            kind: stmt.kind,
            biome_gate: stmt.biome_gate,
        });
        edges.push(Edge {
            a: stmt.anchor,
            b: Anchor::Var(var_idx),
            min: stmt.min,
            max: stmt.max,
        });
    }

    Ok(Query { vars, edges })
}

fn strip_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or("")
}

struct Statement {
    var_name: String,
    kind: VarKind,
    anchor: Anchor,
    min: u32,
    max: u32,
    biome_gate: Option<Vec<String>>,
}

fn parse_statement(
    line: &str,
    lineno: usize,
    declared: &HashMap<String, usize>,
) -> Result<(Statement, usize), DslError> {
    // Split off an optional "; biome=..." gate.
    let (head, gate) = split_gate(line);

    // Tokens: <structure> <var> @<anchor> <range>
    let mut tokens = head.split_whitespace();
    let structure = next(tokens.next(), lineno, "missing structure")?;
    let var_name = next(tokens.next(), lineno, "missing variable name")?;
    let at = next(tokens.next(), lineno, "missing '@' before anchor")?;
    // Accept both "@ origin" (separate) and "@origin" (attached).
    let anchor = if at.starts_with('@') && at.len() > 1 {
        &at[1..]
    } else {
        if at != "@" {
            return Err(DslError::new(
                lineno,
                format!("expected '@' before anchor, got '{at}'"),
            ));
        }
        next(tokens.next(), lineno, "missing anchor")?
    };
    // The range may be one token ("<=800", "in 600..1200") or two ("<= 800",
    // ">= 3000"). Collect the rest and join so both forms work.
    let range_tokens: Vec<&str> = tokens.collect();
    if range_tokens.is_empty() {
        return Err(DslError::new(lineno, "missing range".to_string()));
    }
    let range = range_tokens.join(" ");

    let anchor = match anchor {
        "origin" => Anchor::Origin,
        name => match declared.get(name) {
            Some(&i) => Anchor::Var(i),
            None => {
                return Err(DslError::new(
                    lineno,
                    format!("unknown anchor '{name}' (declare it earlier)"),
                ))
            }
        },
    };

    let (min, max) = parse_range(&range, lineno)?;

    let kind = if structure == "biome" {
        // biome-presence probe; the biome names come from the gate if provided,
        // otherwise from the structure key.
        let biomes = gate.clone().unwrap_or_default();
        VarKind::BiomePresence { biomes }
    } else {
        VarKind::Structure(structure.to_string())
    };

    let var_idx = declared.len();
    Ok((
        Statement {
            var_name: var_name.to_string(),
            kind,
            anchor,
            min,
            max,
            biome_gate: gate,
        },
        var_idx,
    ))
}

fn next<'a>(tok: Option<&'a str>, lineno: usize, what: &str) -> Result<&'a str, DslError> {
    tok.ok_or_else(|| DslError::new(lineno, what.to_string()))
}

fn split_gate(line: &str) -> (&str, Option<Vec<String>>) {
    // The PLAN example writes ", biome=desert"; also accept "; biome=".
    let (head, rest) = if let Some(idx) = line.find(", biome=") {
        (&line[..idx], &line[idx + ", biome=".len()..])
    } else if let Some(idx) = line.find("; biome=") {
        (&line[..idx], &line[idx + "; biome=".len()..])
    } else {
        return (line, None);
    };
    let names: Vec<String> = rest
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (head, Some(names))
}

fn parse_range(src: &str, lineno: usize) -> Result<(u32, u32), DslError> {
    let s = src.trim();
    if let Some(rest) = s.strip_prefix("<=") {
        let max = parse_u32(rest, lineno)?;
        return Ok((0, max));
    }
    if let Some(rest) = s.strip_prefix(">=") {
        let min = parse_u32(rest, lineno)?;
        return Ok((min, u32::MAX));
    }
    if let Some(inner) = s.strip_prefix("in ") {
        if let Some((a, b)) = inner.split_once("..") {
            let a = parse_u32(a, lineno)?;
            let b = parse_u32(b, lineno)?;
            return Ok((a, b));
        }
        return Err(DslError::new(
            lineno,
            "expected 'in A..B' range".to_string(),
        ));
    }
    Err(DslError::new(
        lineno,
        format!("unrecognized range '{src}' (expected <= N, >= N, or in A..B)"),
    ))
}

fn parse_u32(s: &str, lineno: usize) -> Result<u32, DslError> {
    s.trim()
        .parse::<u32>()
        .map_err(|_| DslError::new(lineno, format!("invalid number '{s}'")))
}

/// Serialize a [`Query`] back to DSL text (stable round-trip).
pub fn serialize(query: &Query) -> String {
    let mut out = String::new();
    for (i, var) in query.vars.iter().enumerate() {
        // Each var has exactly one defining edge in the DSL shape.
        let edge = query
            .edges
            .iter()
            .find(|e| e.b == Anchor::Var(i))
            .or_else(|| query.edges.iter().find(|e| e.a == Anchor::Var(i)));

        let (anchor, min, max) = match edge {
            Some(e) if e.b == Anchor::Var(i) => (e.a, e.min, e.max),
            Some(e) if e.a == Anchor::Var(i) => (e.b, e.min, e.max),
            _ => (Anchor::Origin, 0, 0),
        };

        let kind_str = match &var.kind {
            VarKind::Structure(s) => s.clone(),
            VarKind::BiomePresence { .. } => "biome".to_string(),
        };

        let range_str = match (min, max) {
            (0, u32::MAX) => ">= 0".to_string(),
            (0, m) => format!("<= {m}"),
            (n, u32::MAX) => format!(">= {n}"),
            (n, m) => format!("in {n}..{m}"),
        };

        let anchor_str = match anchor {
            Anchor::Origin => "origin".to_string(),
            Anchor::Var(j) => query.vars[j].name.clone(),
        };

        let mut line = format!("{kind_str} {} @ {} {range_str}", var.name, anchor_str);
        if let Some(gate) = &var.biome_gate {
            line.push_str(&format!("; biome={}", gate.join(",")));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// A DSL parse/semantic error with its line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DslError {
    pub lineno: usize,
    pub message: String,
}

impl DslError {
    fn new(lineno: usize, message: String) -> Self {
        Self { lineno, message }
    }
}

impl std::fmt::Display for DslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.lineno, self.message)
    }
}

impl std::error::Error for DslError {}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "\
# An Indiana-Jones adventure (PLAN §3.3)
village          v1  @origin <= 800
desert_pyramid   t1  @v1 in 600..1200, biome=desert
ocean_monument   m1  @t1 <= 1500
woodland_mansion x1  @origin >= 3000
";

    #[test]
    fn parses_plan_example() {
        let q = parse(EXAMPLE).expect("plan example parses");
        assert_eq!(q.vars.len(), 4);
        assert_eq!(q.edges.len(), 4);
        assert_eq!(q.vars[0].name, "v1");
        assert!(matches!(q.vars[0].kind, VarKind::Structure(ref s) if s == "village"));
        assert_eq!(
            q.vars[1].biome_gate.as_deref(),
            Some(&["desert".to_string()][..])
        );
        assert_eq!(q.edges[0].a, Anchor::Origin);
        assert_eq!(q.edges[0].b, Anchor::Var(0));
        assert_eq!((q.edges[0].min, q.edges[0].max), (0, 800));
        assert_eq!((q.edges[1].min, q.edges[1].max), (600, 1200));
        assert_eq!(q.edges[1].a, Anchor::Var(0));
        assert_eq!(q.edges[1].b, Anchor::Var(1));
    }

    #[test]
    fn round_trip_is_stable() {
        let q = parse(EXAMPLE).unwrap();
        let text = serialize(&q);
        let q2 = parse(&text).unwrap();
        assert_eq!(q.vars.len(), q2.vars.len());
        assert_eq!(q.edges.len(), q2.edges.len());
        for (a, b) in q.vars.iter().zip(q2.vars.iter()) {
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.biome_gate, b.biome_gate);
        }
    }

    #[test]
    fn unknown_anchor_is_rejected() {
        let e = parse("village v1 @nowhere <= 800").unwrap_err();
        assert!(e.message.contains("nowhere"));
        assert_eq!(e.lineno, 1);
    }

    #[test]
    fn forward_reference_is_rejected() {
        let e = parse("village v1 @t1 <= 800").unwrap_err();
        assert!(e.message.contains("t1"));
    }

    #[test]
    fn malformed_line_is_rejected() {
        let e = parse("village v1 @origin").unwrap_err();
        assert!(e.message.contains("range"), "got: {}", e.message);
    }

    #[test]
    fn biome_presence_probe_parses() {
        // A standalone biome-presence probe uses the `biome` keyword in place of a
        // structure key: `biome <var> @<anchor> <range>`.
        let q = parse("biome swamp1 @origin <= 1000").unwrap();
        assert!(matches!(q.vars[0].kind, VarKind::BiomePresence { .. }));
        assert_eq!(q.vars[0].name, "swamp1");
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let q = parse("# only a comment\n\n\n   \n").unwrap();
        assert!(q.vars.is_empty());
        assert!(q.edges.is_empty());
    }

    #[test]
    fn three_temples_mutually_close() {
        // The graph the linear builder can't express (PLAN §3.3).
        let dsl = "\
desert_pyramid a @origin <= 2000
desert_pyramid b @a in 0..2000
desert_pyramid c @b in 0..2000
";
        let q = parse(dsl).unwrap();
        assert_eq!(q.vars.len(), 3);
        assert!(q.is_connected());
    }
}
