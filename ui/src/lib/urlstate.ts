// URL-encoded state so seeds and views are shareable (PLAN §3.2).
//
// Encode the seed, the map camera, and the query DSL into a compact query string. The
// camera is encoded with a fixed precision so it round-trips stably across reloads.

import type { Camera } from "./camera";

export interface ViewState {
  seed: string;
  camera: Camera;
  dsl: string;
}

const PREC = 1000; // encode 3 decimals

/** Encode a view state into a query string (no leading '?'). */
export function encode(state: ViewState): string {
  const p = new URLSearchParams();
  p.set("seed", state.seed);
  p.set("cx", Math.round(state.camera.centerX * PREC).toString());
  p.set("cz", Math.round(state.camera.centerZ * PREC).toString());
  p.set("z", state.camera.pxPerBlock.toString());
  if (state.dsl) p.set("q", state.dsl);
  return p.toString();
}

/** Decode a query string into a view state, or null if it carries no view info. */
export function decode(query: string): ViewState | null {
  const p = new URLSearchParams(query);
  const seed = p.get("seed");
  if (seed == null) return null;
  const centerX = parseInt(p.get("cx") ?? "0", 10) / PREC;
  const centerZ = parseInt(p.get("cz") ?? "0", 10) / PREC;
  const pxPerBlock = parseFloat(p.get("z") ?? "1");
  return {
    seed,
    camera: { centerX: isNaN(centerX) ? 0 : centerX, centerZ: isNaN(centerZ) ? 0 : centerZ, pxPerBlock: isNaN(pxPerBlock) || pxPerBlock <= 0 ? 1 : pxPerBlock },
    dsl: p.get("q") ?? "",
  };
}

/** Merge a new state into the current URL without clobbering unrelated params. */
export function applyToUrl(state: ViewState): void {
  const url = new URL(window.location.href);
  const p = new URLSearchParams(encode(state));
  // Preserve unrelated params.
  for (const [k, v] of url.searchParams) {
    if (!p.has(k)) p.set(k, v);
  }
  url.search = p.toString();
  window.history.replaceState(null, "", url.toString());
}
