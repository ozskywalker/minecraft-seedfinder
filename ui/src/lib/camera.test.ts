import { describe, expect, it } from "vitest";
import {
  REBASE_THRESHOLD,
  TILE_BLOCKS,
  lodFor,
  rebase,
  screenToWorld,
  visibleTiles,
  worldToScreen,
} from "./camera";

const VP = { width: 800, height: 600 };

describe("world<->screen", () => {
  it("maps the centre to the viewport centre", () => {
    const cam = { centerX: 1000, centerZ: -500, pxPerBlock: 1 };
    const { sx, sy } = worldToScreen(cam, 1000, -500, VP);
    expect(sx).toBe(VP.width / 2);
    expect(sy).toBe(VP.height / 2);
  });

  it("round-trips", () => {
    const cam = { centerX: 1234, centerZ: -5678, pxPerBlock: 2 };
    for (const [x, z] of [
      [0, 0],
      [100, 200],
      [-50, 77],
    ] as const) {
      const s = worldToScreen(cam, x, z, VP);
      const w = screenToWorld(cam, s.sx, s.sy, VP);
      expect(w.x).toBeCloseTo(x, 3);
      expect(w.z).toBeCloseTo(z, 3);
    }
  });
});

describe("visibleTiles", () => {
  it("lists tiles covering the viewport", () => {
    const cam = { centerX: 0, centerZ: 0, pxPerBlock: 1 };
    // Viewport 800x600 at 1px/block → world bounds ±400 x, ±300 z.
    const tiles = visibleTiles(cam, VP);
    expect(tiles.length).toBeGreaterThan(1);
    for (const t of tiles) {
      expect((t.x0 % TILE_BLOCKS) === 0).toBe(true);
      expect((t.z0 % TILE_BLOCKS) === 0).toBe(true);
    }
    // Tile at origin must be present.
    expect(tiles.some((t) => t.tx === 0 && t.tz === 0)).toBe(true);
  });

  it("covers negative and positive coordinates", () => {
    const cam = { centerX: 256, centerZ: 256, pxPerBlock: 1 };
    const tiles = visibleTiles(cam, VP);
    expect(tiles.some((t) => t.tx < 0)).toBe(true);
    expect(tiles.some((t) => t.tx > 0)).toBe(true);
    expect(tiles.some((t) => t.tz < 0)).toBe(true);
    expect(tiles.some((t) => t.tz > 0)).toBe(true);
  });

  it("tile screen size equals blocks*scale", () => {
    const cam = { centerX: 0, centerZ: 0, pxPerBlock: 2 };
    const tiles = visibleTiles(cam, VP);
    expect(tiles[0]!.size).toBe(TILE_BLOCKS * 2);
  });
});

describe("lodFor", () => {
  it("uses full detail when zoomed in", () => {
    expect(lodFor({ centerX: 0, centerZ: 0, pxPerBlock: 1 })).toBe(0);
  });

  it("coarsens as you zoom out", () => {
    const coarse = lodFor({ centerX: 0, centerZ: 0, pxPerBlock: 0.01 });
    const fine = lodFor({ centerX: 0, centerZ: 0, pxPerBlock: 1 });
    expect(coarse).toBeGreaterThan(fine);
  });

  it("never exceeds the server max", () => {
    const lod = lodFor({ centerX: 0, centerZ: 0, pxPerBlock: 1e-9 });
    expect(lod).toBeLessThanOrEqual(6);
  });
});

describe("rebase", () => {
  it("keeps the base when near the centre", () => {
    const cam = { centerX: 1000, centerZ: 1000, pxPerBlock: 1 };
    const { state } = rebase(cam, { baseX: 0, baseZ: 0 });
    expect(state).toEqual({ baseX: 0, baseZ: 0 });
  });

  it("advances the base beyond the threshold", () => {
    const cam = { centerX: REBASE_THRESHOLD + 1000, centerZ: -REBASE_THRESHOLD - 1000, pxPerBlock: 1 };
    const { cam: local, state } = rebase(cam, { baseX: 0, baseZ: 0 });
    expect(state.baseX).toBe(cam.centerX);
    expect(state.baseZ).toBe(cam.centerZ);
    // Local camera centre is the offset (0 here since we rebased exactly to centre).
    expect(local.centerX).toBeCloseTo(0, 0);
  });
});
