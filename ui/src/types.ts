// Shared UI types.

/** A structure in the catalog returned by /api/catalog. */
export interface CatalogStructure {
  key: string;
  biomes: string[];
  shares_slot_with: string[];
}

export interface Catalog {
  version: string;
  seed_bits: number;
  structures: CatalogStructure[];
}

/** A bound position for one variable in a search result. */
export interface ResultPosition {
  name: string;
  x: number;
  z: number;
}

/** A single search hit streamed from the server. */
export interface SearchResult {
  seed: string;
  positions: ResultPosition[];
}

/** The active search mode + whether it's complete (honesty, PLAN §3.1). */
export interface ModeInfo {
  mode: string;
  complete: boolean;
}
