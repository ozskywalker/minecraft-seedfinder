// TileFetcher — loads server-rendered 512-block PNG tiles with progressive LOD
// (PLAN §3.2: "stretch a coarse tile immediately, swap in the sharp one when ready").
//
// For each (tx, tz) at a requested LOD we first show a coarser tile (stretched) if one
// is available, then the sharp one. In-flight requests are deduped by key so a pan that
// re-enters a tile doesn't double-fetch. Images are cached in an LRU so revisiting a
// view is instant (the server also caches).

export class TileFetcher {
  private cache = new Map<string, HTMLImageElement>();
  private inflight = new Map<string, Promise<HTMLImageElement>>();
  private maxEntries: number;

  constructor(maxEntries = 512, private base = "") {
    this.maxEntries = maxEntries;
  }

  key(seed: string, tx: number, tz: number, lod: number): string {
    return `${seed}:${tx}:${tz}:${lod}`;
  }

  private touch(key: string, img: HTMLImageElement): void {
    this.cache.delete(key);
    this.cache.set(key, img);
    if (this.cache.size > this.maxEntries) {
      const oldest = this.cache.keys().next().value;
      if (oldest) this.cache.delete(oldest);
    }
  }

  getCached(seed: string, tx: number, tz: number, lod: number): HTMLImageElement | undefined {
    return this.cache.get(this.key(seed, tx, tz, lod));
  }

  /** Fetch a tile, returning a promise that resolves to the loaded image. */
  fetch(seed: string, tx: number, tz: number, lod: number): Promise<HTMLImageElement> {
    const k = this.key(seed, tx, tz, lod);
    const cached = this.cache.get(k);
    if (cached) return Promise.resolve(cached);
    const existing = this.inflight.get(k);
    if (existing) return existing;

    const p = new Promise<HTMLImageElement>((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        this.touch(k, img);
        this.inflight.delete(k);
        resolve(img);
      };
      img.onerror = () => {
        this.inflight.delete(k);
        reject(new Error(`tile fetch failed: ${k}`));
      };
      img.src = `${this.base}/api/tile/${seed}/${tx}/${tz}/${lod}`;
    });
    this.inflight.set(k, p);
    return p;
  }

  /** Number of entries currently held. Exposed for tests. */
  size(): number {
    return this.cache.size;
  }
}
