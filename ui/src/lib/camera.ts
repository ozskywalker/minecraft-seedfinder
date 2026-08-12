// Camera + tile math for the map canvas (PLAN §3.2).
//
// The world is flat MC coordinates: +x east (right on screen), +z south (down).
// A tile covers a 512-block square from (tx*512, tz*512) to ((tx+1)*512, (tz+1)*512).
//
// This module is pure (no DOM) so it's unit-testable. The React component owns a Camera
// and asks these functions what to draw.

export interface Camera {
  /** World block x at the screen centre. */
  centerX: number;
  /** World block z at the screen centre. */
  centerZ: number;
  /** Pixels per block (zoom). Larger = more zoomed in. */
  pxPerBlock: number;
}

export interface Viewport {
  width: number;
  height: number;
}

/** Blocks per tile edge (must match the server's tile renderer). */
export const TILE_BLOCKS = 512;
/** Full-detail samples per tile edge (the server returns a 128×128 PNG at lod 0). */
export const TILE_SAMPLES = 128;
/** Max LOD the server accepts. */
export const MAX_LOD = 6;

/** World block → screen pixel (top-left origin; z grows downward). */
export function worldToScreen(cam: Camera, x: number, z: number, vp: Viewport): { sx: number; sy: number } {
  return {
    sx: vp.width / 2 + (x - cam.centerX) * cam.pxPerBlock,
    sy: vp.height / 2 + (z - cam.centerZ) * cam.pxPerBlock,
  };
}

/** Screen pixel → world block. */
export function screenToWorld(cam: Camera, sx: number, sy: number, vp: Viewport): { x: number; z: number } {
  return {
    x: cam.centerX + (sx - vp.width / 2) / cam.pxPerBlock,
    z: cam.centerZ + (sy - vp.height / 2) / cam.pxPerBlock,
  };
}

export interface VisibleTile {
  tx: number;
  tz: number;
  /** World origin of this tile. */
  x0: number;
  z0: number;
  /** Screen top-left of this tile. */
  sx: number;
  sy: number;
  /** Screen size in pixels (one edge). */
  size: number;
}

/**
 * The tiles whose 512-block footprint intersects the viewport.
 * `pxPerBlock` may be < 1 (zoomed out), so a tile can be sub-pixel; still listed once.
 */
export function visibleTiles(cam: Camera, vp: Viewport): VisibleTile[] {
  // World bounds of the viewport.
  const nw = screenToWorld(cam, 0, 0, vp);
  const se = screenToWorld(cam, vp.width, vp.height, vp);
  const minX = Math.floor(Math.min(nw.x, se.x) / TILE_BLOCKS);
  const maxX = Math.floor(Math.max(nw.x, se.x) / TILE_BLOCKS);
  const minZ = Math.floor(Math.min(nw.z, se.z) / TILE_BLOCKS);
  const maxZ = Math.floor(Math.max(nw.z, se.z) / TILE_BLOCKS);
  const size = TILE_BLOCKS * cam.pxPerBlock;
  const out: VisibleTile[] = [];
  for (let tx = minX; tx <= maxX; tx++) {
    for (let tz = minZ; tz <= maxZ; tz++) {
      const x0 = tx * TILE_BLOCKS;
      const z0 = tz * TILE_BLOCKS;
      const { sx, sy } = worldToScreen(cam, x0, z0, vp);
      out.push({ tx, tz, x0, z0, sx, sy, size });
    }
  }
  return out;
}

/**
 * Choose a coarse-to-fine LOD for a given zoom. Higher lod = coarser (fewer samples).
 * The server returns 128px at lod 0, halving each lod. We pick the coarsest lod whose
 * image is at least half the tile's on-screen size (progressive LOD: stretch coarse
 * immediately, swap in sharp later).
 */
export function lodFor(cam: Camera): number {
  const target = 0.25 / cam.pxPerBlock; // image width we want (see module analysis)
  if (target <= 1) return 0;
  const lod = Math.ceil(Math.log2(target));
  return Math.max(0, Math.min(MAX_LOD, lod));
}

export interface RebaseState {
  /** World block the local (float32-safe) frame is anchored to. */
  baseX: number;
  baseZ: number;
}

/** Distance from the base frame beyond which we rebase to keep coords float32-safe. */
export const REBASE_THRESHOLD = 4_000_000;

/**
 * Compute a rebased camera for drawing: returns screen coordinates relative to a base
 * frame so world→screen magnitudes stay within float32 precision even at ±30M blocks
 * and deep zoom. When the centre drifts past `REBASE_THRESHOLD` from `base`, advance the
 * base to the centre.
 */
export function rebase(cam: Camera, state: RebaseState): { cam: Camera; state: RebaseState } {
  let { baseX, baseZ } = state;
  const dx = cam.centerX - baseX;
  const dz = cam.centerZ - baseZ;
  if (Math.abs(dx) > REBASE_THRESHOLD || Math.abs(dz) > REBASE_THRESHOLD) {
    baseX = cam.centerX;
    baseZ = cam.centerZ;
  }
  // Offset (in blocks) from the base to the viewport centre.
  const ox = cam.centerX - baseX;
  const oz = cam.centerZ - baseZ;
  // A "local" camera whose centre is the offset, so screen coords = (local)*scale + half.
  return { cam: { centerX: ox, centerZ: oz, pxPerBlock: cam.pxPerBlock }, state: { baseX, baseZ } };
}

/** The block-space magnitude of a rebased screen point's coordinate (for debugging). */
export function localMagnitude(cam: Camera): number {
  const c = Math.abs(cam.centerX) + Math.abs(cam.centerZ);
  return c * cam.pxPerBlock;
}
