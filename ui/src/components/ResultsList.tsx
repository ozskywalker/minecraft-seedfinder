// ResultsList — the accumulated search hits with their seed + bound positions.
// Single-click selects a result; double-click copies the seed to the clipboard.

import type { SearchResult } from "../types";

interface ResultsListProps {
  results: SearchResult[];
  running: boolean;
  done: boolean;
  onSelect: (r: SearchResult) => void;
  onCopy: (r: SearchResult) => void;
  selectedSeed: string | null;
}

export function ResultsList({ results, running, done, onSelect, onCopy, selectedSeed }: ResultsListProps) {
  return (
    <section className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex items-baseline justify-between">
        <label className="text-xs font-semibold uppercase tracking-wide text-slate-400">Results</label>
        <span className="text-xs text-slate-500">
          {results.length}
          {running ? " (streaming…)" : done ? "" : " (idle)"}
        </span>
      </div>
      {results.length === 0 ? (
        <p className="text-xs text-slate-500">No results yet. Run a search.</p>
      ) : (
        <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto">
          {results.map((r) => (
            <li key={r.seed + r.positions.map((p) => `${p.name}${p.x},${p.z}`).join(";")}>
              <button
                onClick={() => onSelect(r)}
                onDoubleClick={() => onCopy(r)}
                title="Double-click to copy the seed"
                className={`w-full rounded border px-2 py-1 text-left text-xs transition-colors ${
                  selectedSeed === r.seed
                    ? "border-sky-600 bg-sky-950 text-sky-200"
                    : "border-slate-700 bg-slate-800 text-slate-200 hover:bg-slate-700"
                }`}
              >
                <span className="font-mono">{r.seed}</span>
                <span className="ml-2 text-slate-400">
                  {r.positions.map((p) => `${p.name}(${p.x},${p.z})`).join(" · ")}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
