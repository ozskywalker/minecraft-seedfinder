// App — wires the map, search, results, and route builder together, owns the shared
// state (seed, camera, DSL), and syncs the view/seed/query to the URL for sharing.

import { useCallback, useEffect, useRef, useState } from "react";
import { MapCanvas } from "./components/MapCanvas";
import { ResultsList } from "./components/ResultsList";
import { RouteBuilder } from "./components/RouteBuilder";
import { SearchPanel } from "./components/SearchPanel";
import type { Camera } from "./lib/camera";
import { copyText } from "./lib/clipboard";
import { streamSearch, type SearchEvent } from "./lib/sse";
import { applyToUrl, decode } from "./lib/urlstate";
import type { Catalog, ModeInfo, SearchResult } from "./types";

const DEFAULT_SEED = "42";

export function App() {
  // Restore initial state from the URL (seed + camera + DSL are shareable).
  const initial = useMemoInitialState();

  const [seed, setSeed] = useState(initial.seed);
  const [camera, setCamera] = useState<Camera>(initial.camera);
  const [dsl, setDsl] = useState(initial.dsl);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState(false);
  const [mode, setMode] = useState<ModeInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [selectedSeed, setSelectedSeed] = useState<string | null>(null);
  const [copiedSeed, setCopiedSeed] = useState<string | null>(null);

  const abortRef = useRef<AbortController | null>(null);
  const copyTimerRef = useRef<number | null>(null);
  const base = ""; // same origin (dev proxy / production both serve /api)

  // Load the structure catalog once.
  useEffect(() => {
    fetch(`${base}/api/catalog`)
      .then((r) => r.json())
      .then((c: Catalog) => setCatalog(c))
      .catch(() => setCatalog(null));
  }, []);

  // Keep the URL in sync with the current view (throttled-ish: on camera change).
  useEffect(() => {
    try {
      applyToUrl({ seed, camera, dsl });
    } catch {
      /* URL sync is best-effort */
    }
  }, [seed, camera, dsl]);

  const handleEvent = useCallback((ev: SearchEvent) => {
    switch (ev.type) {
      case "mode":
        setMode({ mode: ev.mode, complete: ev.complete });
        break;
      case "result":
        setResults((prev) => [...prev, { seed: ev.seed, positions: ev.positions }]);
        setSelectedSeed(ev.seed);
        break;
      case "done":
        setRunning(false);
        setDone(true);
        break;
      case "note":
        setNote(ev.message);
        setRunning(false);
        setDone(true);
        break;
    }
  }, []);

  async function onRun() {
    if (running) return;
    setError(null);
    setNote(null);
    setMode(null);
    setResults([]);
    setDone(false);
    setRunning(true);
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    try {
      await streamSearch({ dsl }, handleEvent, ctrl.signal, base);
    } catch (e) {
      if ((e as Error).name !== "SearchAbort") {
        setError((e as Error).message);
        setRunning(false);
        setDone(true);
      }
    }
  }

  function onStop() {
    abortRef.current?.abort();
    setRunning(false);
  }

  // Copy a result's seed to the clipboard and flash a confirmation toast. The toast
  // is shown only on a successful copy (never fabricated).
  async function onCopyResult(r: SearchResult) {
    const ok = await copyText(r.seed);
    if (ok) {
      setCopiedSeed(r.seed);
      if (copyTimerRef.current) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopiedSeed(null), 2500);
    }
  }

  // Clear any pending toast timer on unmount.
  useEffect(() => {
    return () => {
      if (copyTimerRef.current) window.clearTimeout(copyTimerRef.current);
    };
  }, []);

  const selected = results.find((r) => r.seed === selectedSeed);
  const shownResults = selected ? [selected] : results.slice(-1);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-slate-800 bg-slate-900 px-4 py-2">
        <h1 className="text-sm font-semibold text-slate-100">Bedrock Seedfinder</h1>
        <label className="text-xs text-slate-400">Seed</label>
        <input
          value={seed}
          onChange={(e) => setSeed(e.target.value.trim())}
          className="w-40 rounded border border-slate-700 bg-slate-800 px-2 py-1 font-mono text-xs text-slate-100"
          placeholder="seed (decimal or hex)"
        />
        {catalog && <span className="text-xs text-slate-500">version {catalog.version}</span>}
      </header>

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-80 flex-col gap-4 overflow-y-auto border-r border-slate-800 bg-slate-900 p-3">
          <SearchPanel
            dsl={dsl}
            onDslChange={setDsl}
            running={running}
            mode={mode}
            error={error}
            note={note}
            onRun={onRun}
            onStop={onStop}
          />
          <RouteBuilder catalog={catalog} dsl={dsl} onApplyDsl={setDsl} />
          <ResultsList
            results={results}
            running={running}
            done={done}
            onSelect={(r) => setSelectedSeed(r.seed)}
            onCopy={onCopyResult}
            selectedSeed={selectedSeed}
          />
        </aside>

        <main className="relative min-w-0 flex-1 bg-slate-950">
          <MapCanvas seed={seed} camera={camera} results={shownResults} onCameraChange={setCamera} />
        </main>
      </div>

      {copiedSeed !== null && (
        <div className="pointer-events-none fixed bottom-4 right-4 z-50 rounded border border-emerald-700 bg-emerald-950 px-3 py-2 text-sm text-emerald-300 shadow-lg">
          Copied seed <span className="font-mono font-semibold text-emerald-200">{copiedSeed}</span> to clipboard
        </div>
      )}
    </div>
  );
}

/** Parse the initial URL state once, falling back to defaults. */
function useMemoInitialState() {
  return useMemoOnce(() => {
    const fromUrl = decode(window.location.search);
    if (fromUrl) return fromUrl;
    return { seed: DEFAULT_SEED, camera: { centerX: 0, centerZ: 0, pxPerBlock: 1 }, dsl: "" };
  });
}

function useMemoOnce<T>(fn: () => T): T {
  const ref = useRef<T | null>(null);
  if (ref.current === null) ref.current = fn();
  return ref.current;
}
