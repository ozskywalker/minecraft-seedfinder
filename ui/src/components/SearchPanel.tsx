// SearchPanel — the text DSL editor + run/stop controls + mode honesty banner.

import type { ModeInfo } from "../types";

interface SearchPanelProps {
  dsl: string;
  onDslChange: (s: string) => void;
  running: boolean;
  mode: ModeInfo | null;
  error: string | null;
  note: string | null;
  onRun: () => void;
  onStop: () => void;
}

export function SearchPanel({ dsl, onDslChange, running, mode, error, note, onRun, onStop }: SearchPanelProps) {
  return (
    <section className="flex flex-col gap-2">
      <label className="text-xs font-semibold uppercase tracking-wide text-slate-400">Query (DSL)</label>
      <textarea
        value={dsl}
        onChange={(e) => onDslChange(e.target.value)}
        spellCheck={false}
        rows={8}
        className="w-full resize-y rounded border border-slate-700 bg-slate-800 p-2 font-mono text-sm text-slate-100 focus:border-sky-500 focus:outline-none"
        placeholder={"village        v1 @origin <= 800\ndesert_pyramid t1 @v1 in 600..1200, biome=desert"}
      />

      <div className="flex gap-2">
        <button
          onClick={onRun}
          disabled={running || dsl.trim().length === 0}
          className="flex-1 rounded bg-sky-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {running ? "Searching…" : "Run search"}
        </button>
        <button
          onClick={onStop}
          disabled={!running}
          className="rounded bg-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-600 disabled:cursor-not-allowed disabled:opacity-40"
        >
          Stop
        </button>
      </div>

      {mode && (
        <div
          className={`rounded border px-2 py-1 text-xs ${
            mode.complete
              ? "border-emerald-700 bg-emerald-950 text-emerald-300"
              : "border-amber-700 bg-amber-950 text-amber-300"
          }`}
        >
          Mode: <span className="font-semibold">{mode.mode}</span>
          {mode.complete ? " — exhaustive, complete over the structural subspace" : " — satisficing, no completeness guarantee"}
        </div>
      )}

      {error && <div className="rounded border border-red-800 bg-red-950 px-2 py-1 text-xs text-red-300">{error}</div>}
      {note && <div className="rounded border border-sky-800 bg-sky-950 px-2 py-1 text-xs text-sky-300">{note}</div>}
    </section>
  );
}
