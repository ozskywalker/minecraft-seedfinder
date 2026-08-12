// DSL module — mirrors `be-search/src/dsl.rs` (Rust) exactly.
//
// The route builder compiles to this DSL text and the server parses it, so the format
// must match the Rust grammar bit-for-bit:
//
//   <structure> <var> @ <anchor> <range> [; biome=<b1>,<b2>]
//
//   - structure: version-table key, or `biome` for a biome-presence probe
//   - var:       a human variable name (e.g. "v1")
//   - anchor:    `origin` or a previously-declared variable name
//   - range:     `<= N`, `>= N`, or `in A..B` (inclusive blocks)
//   - biome=:    optional per-structure biome gate (comma-separated)
//
// `serialize`/`parse` round-trip, matching the Rust `serialize` output format so a
// route started visually can graduate to text (and back) without losing work.

export const MAX_DIST = 4294967295; // u32::MAX

export interface Step {
  /** Variable name, e.g. "v1". */
  name: string;
  /** Version-table structure key, or "biome" for a biome-presence probe. */
  structure: string;
  /** Anchor: "origin" or a previously-declared variable name. */
  anchor: string;
  /** Inclusive minimum distance in blocks. */
  min: number;
  /** Inclusive maximum distance in blocks. */
  max: number;
  /** Per-structure biome gate (empty = none). */
  biomeGate: string[];
}

export class DslError extends Error {
  constructor(
    public readonly lineno: number,
    message: string,
  ) {
    super(message);
    this.name = "DslError";
  }
}

/** Format a (min, max) pair into the DSL range token, matching Rust `serialize`. */
export function formatRange(min: number, max: number): string {
  if (min === 0 && max === MAX_DIST) return ">= 0";
  if (min === 0) return `<= ${max}`;
  if (max === MAX_DIST) return `>= ${min}`;
  return `in ${min}..${max}`;
}

/** Serialize a list of steps into DSL text (stable round-trip, Rust-compatible). */
export function serialize(steps: Step[]): string {
  return steps.map((s) => {
    let line = `${s.structure} ${s.name} @ ${s.anchor} ${formatRange(s.min, s.max)}`;
    if (s.biomeGate.length > 0) line += `; biome=${s.biomeGate.join(",")}`;
    return line;
  }).join("\n");
}

function stripComment(line: string): string {
  return line.split("#")[0]!;
}

function splitGate(line: string): { head: string; gate: string[] } {
  const m = line.match(/(?:, |; )biome=(.*)$/);
  if (!m) return { head: line, gate: [] };
  const gate = m[1]!
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return { head: line.slice(0, line.indexOf(m[0]!)).trimEnd(), gate };
}

function parseRange(src: string, lineno: number): [number, number] {
  const s = src.trim();
  if (s.startsWith("<=")) {
    return [0, parseNum(s.slice(2), lineno)];
  }
  if (s.startsWith(">=")) {
    return [parseNum(s.slice(2), lineno), MAX_DIST];
  }
  if (s.startsWith("in ")) {
    const inner = s.slice(3).trim();
    const idx = inner.indexOf("..");
    if (idx === -1) throw new DslError(lineno, "expected 'in A..B' range");
    return [parseNum(inner.slice(0, idx), lineno), parseNum(inner.slice(idx + 2), lineno)];
  }
  throw new DslError(lineno, `unrecognized range '${src}' (expected <= N, >= N, or in A..B)`);
}

function parseNum(s: string, lineno: number): number {
  const n = Number(s.trim());
  if (!Number.isFinite(n) || n < 0 || !Number.isInteger(n)) {
    throw new DslError(lineno, `invalid number '${s}'`);
  }
  return n;
}

/**
 * Parse DSL text into ordered steps. Each line declares a variable anchored to origin
 * or a *previously declared* variable (forward references are rejected, as in Rust).
 */
export function parse(input: string): Step[] {
  const steps: Step[] = [];
  const declared = new Set<string>(["origin"]);
  const lines = input.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = stripComment(lines[i]!).trim();
    if (line.length === 0) continue;
    const lineno = i + 1;
    const { head, gate } = splitGate(line);
    const tokens = head.trim().split(/\s+/);
    if (tokens.length < 4) {
      throw new DslError(lineno, `malformed statement: '${head}'`);
    }
    const structure = tokens[0]!;
    const name = tokens[1]!;
    // Anchor token may be "@origin" (attached) or "@" then "origin" (separate).
    let anchor: string;
    let rangeTokens: string[];
    if (tokens[2]!.startsWith("@") && tokens[2]!.length > 1) {
      anchor = tokens[2]!.slice(1);
      rangeTokens = tokens.slice(3);
    } else if (tokens[2] === "@") {
      anchor = tokens[3]!;
      rangeTokens = tokens.slice(4);
    } else {
      throw new DslError(lineno, `expected '@' before anchor, got '${tokens[2]}'`);
    }
    if (declared.has(name)) {
      throw new DslError(lineno, `variable '${name}' already declared`);
    }
    if (!declared.has(anchor)) {
      throw new DslError(lineno, `unknown anchor '${anchor}' (declare it earlier)`);
    }
    if (rangeTokens.length === 0) {
      throw new DslError(lineno, "missing range");
    }
    const [min, max] = parseRange(rangeTokens.join(" "), lineno);
    declared.add(name);
    steps.push({ name, structure, anchor, min, max, biomeGate: gate });
  }
  return steps;
}

/** Parse-and-reserialize stability check (used by tests + the round-trip UI path). */
export function roundTrip(text: string): string {
  return serialize(parse(text));
}
