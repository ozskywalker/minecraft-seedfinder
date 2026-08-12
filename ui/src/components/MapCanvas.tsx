// MapCanvas — renders server-side biome tiles, pans/zooms, and draws structure markers.
//
// It keeps the tile cache + canvas drawing in `useRef`s so pan/zoom doesn't re-fetch or
// re-render React on every frame; only camera changes (which the parent owns, for URL
// sync) trigger a redraw. Progressive LOD: show a coarse tile stretched, then the sharp
// one.

import { useEffect, useRef } from "react";
import {
  type Camera,
  type VisibleTile,
  lodFor,
  rebase,
  visibleTiles,
  worldToScreen,
} from "../lib/camera";
import { decluster } from "../lib/decluster";
import { TileFetcher } from "../lib/tileFetcher";
import type { SearchResult } from "../types";

interface MapCanvasProps {
  seed: string;
  camera: Camera;
  results: SearchResult[];
  onCameraChange: (c: Camera) => void;
  /** Base URL for the API (empty = same origin). */
  base?: string;
}

interface DrawState {
  fetcher: TileFetcher;
  ctx: CanvasRenderingContext2D | null;
  raf: number;
}

const TILE_COLOR = "#111827";

export function MapCanvas({ seed, camera, results, onCameraChange, base = "" }: MapCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const stateRef = useRef<DrawState | null>(null);
  // Refs mirror props so the draw loop reads fresh values without re-subscribing.
  const propsRef = useRef({ seed, camera, results });
  propsRef.current = { seed, camera, results };

  // Initialise the fetcher + context once, and size the canvas to its CSS box.
  useEffect(() => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext("2d");
    stateRef.current = { fetcher: new TileFetcher(512, base), ctx, raf: 0 };

    function size() {
      const dpr = window.devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();
      canvas.width = Math.max(1, Math.round(rect.width * dpr));
      canvas.height = Math.max(1, Math.round(rect.height * dpr));
      const ctx2 = canvas.getContext("2d");
      if (ctx2) ctx2.setTransform(dpr, 0, 0, dpr, 0, 0);
      scheduleDraw();
    }
    size();
    const ro = new ResizeObserver(size);
    ro.observe(canvas);

    return () => {
      ro.disconnect();
      if (stateRef.current) cancelAnimationFrame(stateRef.current.raf);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Redraw whenever the camera/seed/results change.
  useEffect(() => {
    scheduleDraw();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [camera, seed, results]);

  function scheduleDraw() {
    const st = stateRef.current;
    if (!st) return;
    cancelAnimationFrame(st.raf);
    st.raf = requestAnimationFrame(() => draw(st));
  }

  function draw(st: DrawState) {
    const canvas = canvasRef.current;
    const ctx = st.ctx;
    if (!canvas || !ctx) return;
    const { seed, camera, results } = propsRef.current;
    // The ctx transform maps CSS px -> device px (dpr), so use CSS dimensions here.
    const vp = { width: canvas.clientWidth || canvas.width, height: canvas.clientHeight || canvas.height };

    // Precision rebase: keep local coords float32-safe at deep zoom / far coords.
    const { cam: local } = rebase(camera, { baseX: 0, baseZ: 0 });
    const drawCam: Camera = local;

    ctx.fillStyle = TILE_COLOR;
    ctx.fillRect(0, 0, vp.width, vp.height);

    const tiles = visibleTiles(drawCam, vp);
    const lod = lodFor(drawCam);
    // Draw tiles (progressive LOD: coarse first if the sharp one isn't loaded).
    for (const t of tiles) drawTile(st, seed, t, lod);

    drawMarkers(ctx, drawCam, results, vp);

    // Coordinate readout (top-left).
    ctx.fillStyle = "rgba(0,0,0,0.6)";
    ctx.fillRect(0, 0, 220, 20);
    ctx.fillStyle = "#e2e8f0";
    ctx.font = "12px ui-monospace, monospace";
    ctx.fillText(
      `cx ${camera.centerX.toFixed(0)} cz ${camera.centerZ.toFixed(0)} · ${lodFor(camera)} lod · ${results.length} hits`,
      4,
      14,
    );
  }

  function drawTile(st: DrawState, seed: string, t: VisibleTile, lod: number) {
    const ctx = st.ctx!;
    const sharp = st.fetcher.getCached(seed, t.tx, t.tz, lod);
    const img = sharp ?? st.fetcher.getCached(seed, t.tx, t.tz, lod + 1);
    if (img) {
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(img, t.sx, t.sy, t.size, t.size);
      // If we only had the coarse one, request the sharp version for a later swap.
      if (!sharp) void st.fetcher.fetch(seed, t.tx, t.tz, lod).then(() => scheduleDraw());
    } else {
      // Nothing cached yet: request coarse (stretch immediately) and sharp.
      void st.fetcher.fetch(seed, t.tx, t.tz, lod + 1).then(() => scheduleDraw());
      void st.fetcher.fetch(seed, t.tx, t.tz, lod).then(() => scheduleDraw());
      ctx.fillStyle = "#1e293b";
      ctx.fillRect(t.sx, t.sy, t.size, t.size);
    }
  }

  function drawMarkers(
    ctx: CanvasRenderingContext2D,
    drawCam: Camera,
    results: SearchResult[],
    vp: { width: number; height: number },
  ) {
    if (results.length === 0) return;
    // Build markers from the *latest* result's positions (the one currently selected /
    // most recently received). We render the most recent result's full route.
    const last = results[results.length - 1]!;
    const markers = last.positions.map((p) => ({ x: p.x, z: p.z, label: p.name }));

    const placed = decluster(markers, (x, z) => worldToScreen(drawCam, x, z, vp), drawCam.pxPerBlock);
    for (const m of placed) {
      const { sx, sy } = worldToScreen(drawCam, m.x, m.z, vp);
      const cx = sx + m.ox;
      const cy = sy + m.oz;
      ctx.fillStyle = "#f59e0b";
      ctx.beginPath();
      ctx.arc(cx, cy, 5, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#0f172a";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = "#fff";
      ctx.font = "11px ui-sans-serif, sans-serif";
      ctx.fillText(m.label, cx + 7, cy + 4);
    }
  }

  // --- Interaction: pan + zoom via pointer/wheel ---
  const dragRef = useRef<{ startX: number; startY: number; cam: Camera } | null>(null);

  function onPointerDown(e: React.PointerEvent) {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragRef.current = { startX: e.clientX, startY: e.clientY, cam: camera };
  }

  function onPointerMove(e: React.PointerEvent) {
    const d = dragRef.current;
    if (!d) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    onCameraChange({
      centerX: d.cam.centerX - dx / d.cam.pxPerBlock,
      centerZ: d.cam.centerZ - dy / d.cam.pxPerBlock,
      pxPerBlock: d.cam.pxPerBlock,
    });
  }

  function onPointerUp() {
    dragRef.current = null;
  }

  function onWheel(e: React.WheelEvent) {
    const rect = canvasRef.current!.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const factor = e.deltaY < 0 ? 1.2 : 1 / 1.2;
    const px = camera.pxPerBlock * factor;
    // Zoom about the cursor.
    const worldX = camera.centerX + (mx - rect.width / 2) / camera.pxPerBlock;
    const worldZ = camera.centerZ + (my - rect.height / 2) / camera.pxPerBlock;
    onCameraChange({
      pxPerBlock: Math.min(20, Math.max(0.01, px)),
      centerX: worldX - (mx - rect.width / 2) / px,
      centerZ: worldZ - (my - rect.height / 2) / px,
    });
  }

  return (
    <canvas
      ref={canvasRef}
      className="h-full w-full touch-none select-none"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onWheel={onWheel}
    />
  );
}
