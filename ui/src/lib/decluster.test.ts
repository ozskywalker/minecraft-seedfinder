import { describe, expect, it } from "vitest";
import { cellPx, decluster, minSpacingPx } from "./decluster";

function toScreenIdentity(x: number, z: number) {
  return { sx: x, sy: z };
}

describe("cellPx / minSpacingPx", () => {
  it("grows as you zoom out", () => {
    expect(cellPx(1)).toBeGreaterThanOrEqual(cellPx(4));
    expect(minSpacingPx(0.01)).toBeGreaterThan(minSpacingPx(1));
  });
});

describe("decluster", () => {
  it("keeps isolated markers at their position (no jitter)", () => {
    const markers = [{ x: 100, z: 100, label: "a" }];
    const placed = decluster(markers, toScreenIdentity, 1);
    expect(placed).toHaveLength(1);
    expect(placed[0]!.ox).toBeCloseTo(0, 3);
    expect(placed[0]!.oz).toBeCloseTo(0, 3);
  });

  it("spreads crowded markers apart", () => {
    // Four markers at nearly the same screen point.
    const markers = [
      { x: 100, z: 100, label: "a" },
      { x: 100, z: 100, label: "b" },
      { x: 100, z: 100, label: "c" },
      { x: 100, z: 100, label: "d" },
    ];
    const placed = decluster(markers, toScreenIdentity, 1);
    expect(placed).toHaveLength(4);
    // At least one is jittered off-centre.
    expect(placed.some((p) => Math.abs(p.ox) > 1e-6 || Math.abs(p.oz) > 1e-6)).toBe(true);
    // All distinct offsets.
    const keys = new Set(placed.map((p) => `${p.ox},${p.oz}`));
    expect(keys.size).toBe(4);
  });

  it("groups by cell so far-apart markers stay put", () => {
    const markers = [
      { x: 0, z: 0, label: "a" },
      { x: 5000, z: 0, label: "b" },
    ];
    const placed = decluster(markers, toScreenIdentity, 1);
    expect(placed).toHaveLength(2);
  });
});
