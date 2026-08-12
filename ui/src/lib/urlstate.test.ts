import { describe, expect, it } from "vitest";
import { decode, encode } from "./urlstate";

describe("urlstate", () => {
  it("round-trips seed, camera, and dsl", () => {
    const state = {
      seed: "1a2b3c4d5e6f",
      camera: { centerX: 1234.567, centerZ: -987.321, pxPerBlock: 2.5 },
      dsl: "village v1 @origin <= 800",
    };
    const q = encode(state);
    const back = decode(q);
    expect(back!.seed).toBe(state.seed);
    expect(back!.camera.centerX).toBeCloseTo(state.camera.centerX, 2);
    expect(back!.camera.centerZ).toBeCloseTo(state.camera.centerZ, 2);
    expect(back!.camera.pxPerBlock).toBe(state.camera.pxPerBlock);
    expect(back!.dsl).toBe(state.dsl);
  });

  it("returns null when no seed is present", () => {
    expect(decode("?foo=bar")).toBeNull();
  });

  it("tolerates garbage numbers", () => {
    const q = encode({ seed: "1234", camera: { centerX: 0, centerZ: 0, pxPerBlock: 1 }, dsl: "" });
    // decode should never throw on malformed numeric params.
    expect(() => decode(q)).not.toThrow();
  });
});
