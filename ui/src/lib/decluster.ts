// Structure-icon declustering (PLAN §3.2: "zoom-dependent declustering — icons clump
// illegibly at low zoom").
//
// Simple deterministic grid decluster: group markers by a screen-space cell, then spread
// the members of a crowded cell around the cell centre so icons/labels don't overlap.
// Cell size grows with zoom-out so the layout stays legible as more structures crowd in.

export interface Marker {
  /** World x. */
  x: number;
  /** World z. */
  z: number;
  /** Short label (structure name / variable). */
  label: string;
}

export interface PlacedMarker {
  x: number;
  z: number;
  label: string;
  /** Screen-space jitter offset (px) from the raw position, for drawing. */
  ox: number;
  oz: number;
}

/** Screen-space cell size in pixels at a given zoom (larger when zoomed out). */
export function cellPx(pxPerBlock: number): number {
  // At high zoom structures are spread out already; at low zoom many share a small
  // screen area, so use a bigger cell to spread them further.
  const base = 28;
  const zoomOutBoost = Math.max(0, Math.log2(1 / Math.max(pxPerBlock, 1e-6)));
  return base + zoomOutBoost * 14;
}

/** Minimum centre-to-centre spacing (px) between declustered icons. */
export function minSpacingPx(pxPerBlock: number): number {
  return cellPx(pxPerBlock) * 0.7;
}

/** Spread offsets for `n` members of a crowded cell (deterministic ring). */
function spreadFor(n: number): { ox: number; oz: number }[] {
  const r = 7;
  const out: { ox: number; oz: number }[] = [];
  for (let i = 0; i < n; i++) {
    const ang = (2 * Math.PI * i) / Math.max(1, n);
    out.push({ ox: r * Math.cos(ang), oz: r * Math.sin(ang) });
  }
  return out;
}

/**
 * Given world-space markers, their screen positions, and the current zoom, return
 * declustered positions with small screen offsets so icons do not overlap.
 */
export function decluster(
  markers: Marker[],
  toScreen: (x: number, z: number) => { sx: number; sy: number },
  pxPerBlock: number,
): PlacedMarker[] {
  const cell = cellPx(pxPerBlock);
  // Group by integer cell key.
  const cells = new Map<string, { sx: number; sy: number; members: Marker[] }>();
  for (const m of markers) {
    const { sx, sy } = toScreen(m.x, m.z);
    const cx = Math.floor(sx / cell);
    const cy = Math.floor(sy / cell);
    const key = `${cx}:${cy}`;
    const cellEntry = cells.get(key) ?? { sx: cx * cell + cell / 2, sy: cy * cell + cell / 2, members: [] };
    cellEntry.members.push(m);
    cells.set(key, cellEntry);
  }
  const out: PlacedMarker[] = [];
  for (const c of cells.values()) {
    const n = c.members.length;
    // A lone marker keeps its exact position; only crowded cells get spread out.
    if (n === 1) {
      const m = c.members[0]!;
      out.push({ x: m.x, z: m.z, label: m.label, ox: 0, oz: 0 });
      continue;
    }
    const spread = spreadFor(n);
    c.members.forEach((m, i) => {
      out.push({ x: m.x, z: m.z, label: m.label, ox: spread[i]!.ox, oz: spread[i]!.oz });
    });
  }
  return out;
}
