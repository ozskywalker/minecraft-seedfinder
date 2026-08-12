// RouteBuilder — the default visual authoring surface (PLAN §3.3).
//
// An ordered waypoint chain: each waypoint is a structure anchored to origin or a
// previous waypoint, with a min–max distance and an optional biome gate. It compiles to
// DSL text (via `serialize`) that the server parses, and can load an existing DSL chain
// back into the builder (`parse`) so the two surfaces round-trip.

import { useState } from "react";
import { MAX_DIST, type Step, parse, serialize } from "../lib/dsl";
import type { Catalog } from "../types";

interface Waypoint {
  id: number;
  name: string;
  structure: string;
  anchor: string;
  min: number;
  max: number;
  biomeGate: string;
}

interface RouteBuilderProps {
  catalog: Catalog | null;
  dsl: string;
  onApplyDsl: (dsl: string) => void;
}

let nextId = 1;

export function RouteBuilder({ catalog, dsl, onApplyDsl }: RouteBuilderProps) {
  const [steps, setSteps] = useState<Waypoint[]>([]);

  const structures = catalog?.structures.map((s) => s.key) ?? [];

  function addWaypoint() {
    const idx = steps.length + 1;
    const anchor = steps.length === 0 ? "origin" : steps[steps.length - 1]!.name;
    setSteps([
      ...steps,
      {
        id: nextId++,
        name: `w${idx}`,
        structure: structures[0] ?? "village",
        anchor,
        min: 0,
        max: 800,
        biomeGate: "",
      },
    ]);
  }

  function update(id: number, patch: Partial<Waypoint>) {
    setSteps(steps.map((s) => (s.id === id ? { ...s, ...patch } : s)));
  }

  function remove(id: number) {
    setSteps(steps.filter((s) => s.id !== id));
  }

  /** The list of anchor options: origin + all prior waypoints. */
  function anchorsUpTo(i: number): string[] {
    return ["origin", ...steps.slice(0, i).map((s) => s.name)];
  }

  function toSteps(): Step[] {
    return steps.map((s) => ({
      name: s.name,
      structure: s.structure,
      anchor: s.anchor,
      min: s.min,
      max: s.max,
      biomeGate: s.biomeGate ? s.biomeGate.split(",").map((x) => x.trim()).filter(Boolean) : [],
    }));
  }

  function generate() {
    if (steps.length === 0) return;
    onApplyDsl(serialize(toSteps()));
  }

  function loadFromDsl() {
    try {
      setSteps(parse(dsl).map((s) => ({ ...s, id: nextId++, biomeGate: s.biomeGate.join(",") })));
    } catch (e) {
      alert((e as Error).message);
    }
  }

  function clear() {
    setSteps([]);
  }

  return (
    <section className="flex flex-col gap-2">
      <label className="text-xs font-semibold uppercase tracking-wide text-slate-400">Route builder</label>

      {steps.length === 0 && (
        <p className="text-xs text-slate-500">
          Build an ordered chain of structures, then "Generate DSL" to run it.
        </p>
      )}

      <div className="space-y-2">
        {steps.map((s, i) => (
          <div key={s.id} className="flex flex-wrap items-center gap-1 rounded border border-slate-700 bg-slate-800 p-1.5 text-xs">
            <span className="w-5 text-slate-500">{i + 1}.</span>
            <select
              value={s.structure}
              onChange={(e) => update(s.id, { structure: e.target.value })}
              className="rounded border border-slate-600 bg-slate-700 px-1 py-0.5"
            >
              {structures.map((st) => (
                <option key={st} value={st}>
                  {st}
                </option>
              ))}
            </select>
            <select
              value={s.anchor}
              onChange={(e) => update(s.id, { anchor: e.target.value })}
              className="rounded border border-slate-600 bg-slate-700 px-1 py-0.5"
            >
              {anchorsUpTo(i).map((a) => (
                <option key={a} value={a}>
                  @{a}
                </option>
              ))}
            </select>
            <span className="text-slate-400">in</span>
            <input
              type="number"
              value={s.min}
              min={0}
              onChange={(e) => update(s.id, { min: Number(e.target.value) })}
              className="w-20 rounded border border-slate-600 bg-slate-700 px-1 py-0.5"
            />
            <span className="text-slate-400">..</span>
            <input
              type="number"
              value={s.max === MAX_DIST ? "" : s.max}
              min={0}
              placeholder="∞"
              onChange={(e) => update(s.id, { max: e.target.value === "" ? MAX_DIST : Number(e.target.value) })}
              className="w-20 rounded border border-slate-600 bg-slate-700 px-1 py-0.5"
            />
            <input
              value={s.biomeGate}
              onChange={(e) => update(s.id, { biomeGate: e.target.value })}
              placeholder="biome: desert"
              className="w-28 rounded border border-slate-600 bg-slate-700 px-1 py-0.5"
            />
            <button
              onClick={() => remove(s.id)}
              className="ml-auto rounded bg-slate-700 px-1.5 text-slate-300 hover:bg-red-800"
              title="Remove"
            >
              ✕
            </button>
          </div>
        ))}
      </div>

      <div className="flex gap-2">
        <button onClick={addWaypoint} className="rounded bg-slate-700 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600">
          + Waypoint
        </button>
        <button
          onClick={generate}
          disabled={steps.length === 0}
          className="rounded bg-sky-700 px-2 py-1 text-xs text-white hover:bg-sky-600 disabled:opacity-40"
        >
          Generate DSL
        </button>
        <button onClick={loadFromDsl} className="rounded bg-slate-700 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600">
          Load from DSL
        </button>
        <button onClick={clear} className="rounded bg-slate-700 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600">
          Clear
        </button>
      </div>
    </section>
  );
}
