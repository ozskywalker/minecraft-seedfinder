import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TileFetcher } from "./tileFetcher";

// Fake Image whose onload fires immediately so fetch() resolves without a browser.
class FakeImage {
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  src = "";
  constructor() {
    const self = this;
    // Set src via assignment in TileFetcher; we capture it via a setter on the class.
    (this as unknown as { src: string }).src = "";
    setTimeout(() => self.onload?.(), 0);
    FakeImage.instances.push(this);
  }
  static instances: FakeImage[] = [];
}

beforeEach(() => {
  FakeImage.instances = [];
  (globalThis as Record<string, unknown>).Image = FakeImage;
  (globalThis as Record<string, unknown>).window = {}; // not needed but harmless
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("TileFetcher", () => {
  it("dedupes concurrent fetches of the same tile", async () => {
    const f = new TileFetcher(16, "");
    const a = f.fetch("42", 0, 0, 0);
    const b = f.fetch("42", 0, 0, 0);
    await Promise.all([a, b]);
    expect(FakeImage.instances.length).toBe(1);
  });

  it("caches and serves from cache", async () => {
    const f = new TileFetcher(16, "");
    const img = await f.fetch("42", 0, 0, 0);
    expect(f.getCached("42", 0, 0, 0)).toBe(img);
    const again = await f.fetch("42", 0, 0, 0);
    expect(again).toBe(img);
  });

  it("evicts oldest entries beyond capacity", async () => {
    const f = new TileFetcher(2, "");
    const first = await f.fetch("42", 0, 0, 0);
    await f.fetch("42", 0, 1, 0);
    await f.fetch("42", 1, 0, 0);
    expect(f.size()).toBeLessThanOrEqual(2);
    expect(f.getCached("42", 0, 0, 0)).toBeUndefined();
    expect(first).toBeDefined();
  });
});
