import { afterEach, describe, expect, it, vi } from "vitest";
import { streamSearch } from "./sse";

// Minimal Response/ReadableStream stand-ins so fetch can be mocked in a node env.
function jsonResponse(body: string): Response {
  return new Response(body, { status: 200, headers: { "content-type": "application/json" } });
}

function sseResponse(frames: string): Response {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(frames));
      controller.close();
    },
  });
  return new Response(stream, { status: 200, headers: { "content-type": "text/event-stream" } });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("streamSearch", () => {
  it("rejects with the server's parse-error message on HTTP 400", async () => {
    const resp = new Response("line 1: unknown anchor 'v1' (declare it earlier)", { status: 400 });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(resp));

    await expect(
      streamSearch({ dsl: "desert_pyramid t1 @v1 in 600..1200, biome=desert" }, () => {}),
    ).rejects.toThrow("search request failed (HTTP 400): line 1: unknown anchor 'v1' (declare it earlier)");
  });

  it("keeps a generic message when the error body is empty", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("", { status: 400 })));
    await expect(streamSearch({ dsl: "x" }, () => {})).rejects.toThrow(
      "search request failed (HTTP 400)",
    );
  });

  it("parses streamed events and resolves on done", async () => {
    const frames = [
      'data: {"type":"mode","mode":"exhaustive","complete":true}\n\n',
      'data: {"type":"result","seed":"1","positions":[]}\n\n',
      'data: {"type":"done","count":1}\n\n',
    ].join("");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(sseResponse(frames)));

    const seen: string[] = [];
    await streamSearch({ dsl: "village v1 @origin <= 800" }, (ev) => seen.push(ev.type));
    expect(seen).toEqual(["mode", "result", "done"]);
  });

  it("uses the supplied base URL", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse("{}"));
    vi.stubGlobal("fetch", fetchMock);
    await expect(streamSearch({ dsl: "x" }, () => {}, undefined, "http://base")).rejects.toThrow();
    const called = fetchMock.mock.calls[0]![0];
    expect(String(called)).toBe("http://base/api/search");
  });
});
