// SSE search client — consumes `POST /api/search` which streams mode → results → done.
//
// The browser `EventSource` API only supports GET, but our search is a POST with a JSON
// body, so we drive the stream manually with `fetch` + a ReadableStream body reader and
// parse `data:` lines into JSON events.

export interface SearchRequest {
  dsl: string;
  low_start?: number;
  low_end?: number;
  high_start?: number;
  high_end?: number;
  max_per_candidate?: number;
  include_biomes?: boolean;
}

export type SearchEvent =
  | { type: "mode"; mode: string; complete: boolean }
  | { type: "result"; seed: string; positions: { name: string; x: number; z: number }[] }
  | { type: "done"; count: number }
  | { type: "note"; message: string };

export type EventHandler = (ev: SearchEvent) => void;

export class SearchAbort extends Error {
  constructor() {
    super("search aborted");
    this.name = "SearchAbort";
  }
}

function parseEventBlock(block: string): SearchEvent | null {
  const data = block
    .split("\n")
    .filter((l) => l.startsWith("data:"))
    .map((l) => l.slice(5).trim())
    .join("\n")
    .trim();
  if (!data) return null;
  return JSON.parse(data) as SearchEvent;
}

/**
 * Stream a search. Calls `onEvent` for each parsed SSE event; resolves when the server
 * sends `done`. Throws `SearchAbort` if `signal` fires first. Any HTTP error or malformed
 * stream rejects with an Error.
 */
export async function streamSearch(
  req: SearchRequest,
  onEvent: EventHandler,
  signal?: AbortSignal,
  base = "",
): Promise<void> {
  const resp = await fetch(`${base}/api/search`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "text/event-stream" },
    body: JSON.stringify(req),
    signal,
  });
  if (!resp.ok || !resp.body) {
    throw new Error(`search request failed (HTTP ${resp.status})`);
  }

  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let doneSeen = false;

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      // SSE frames are separated by a blank line.
      let idx: number;
      while ((idx = buffer.indexOf("\n\n")) !== -1) {
        const block = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);
        const ev = parseEventBlock(block);
        if (!ev) continue;
        onEvent(ev);
        if (ev.type === "done") {
          doneSeen = true;
          // Drain the rest so the connection closes cleanly.
          await reader.cancel();
          return;
        }
      }
    }
    if (!doneSeen) {
      // Stream ended without a done event (e.g. connection dropped).
      throw new Error("search stream ended before done");
    }
  } catch (e) {
    if (signal?.aborted) throw new SearchAbort();
    throw e;
  } finally {
    reader.releaseLock();
  }
}
