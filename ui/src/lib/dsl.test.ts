import { describe, expect, it } from "vitest";
import { DslError, MAX_DIST, parse, roundTrip, serialize } from "./dsl";

const EXAMPLE = `# An Indiana-Jones adventure (PLAN §3.3)
village          v1  @origin <= 800
desert_pyramid   t1  @v1 in 600..1200, biome=desert
ocean_monument   m1  @t1 <= 1500
woodland_mansion x1  @origin >= 3000
`;

describe("parse", () => {
  it("parses the plan example", () => {
    const steps = parse(EXAMPLE);
    expect(steps).toHaveLength(4);
    expect(steps[0]).toMatchObject({ name: "v1", structure: "village", anchor: "origin", min: 0, max: 800 });
    expect(steps[1]).toMatchObject({ structure: "desert_pyramid", anchor: "v1", min: 600, max: 1200 });
    expect(steps[1].biomeGate).toEqual(["desert"]);
    expect(steps[3]).toMatchObject({ anchor: "origin", min: 3000, max: MAX_DIST });
  });

  it("ignores comments and blank lines", () => {
    expect(parse("# only a comment\n\n   \n")).toEqual([]);
  });

  it("rejects forward references", () => {
    expect(() => parse("village v1 @t1 <= 800")).toThrow(DslError);
  });

  it("rejects unknown anchors", () => {
    expect(() => parse("village v1 @nowhere <= 800")).toThrow(DslError);
  });

  it("rejects missing range", () => {
    expect(() => parse("village v1 @origin")).toThrow(DslError);
  });

  it("rejects duplicate variables", () => {
    expect(() => parse("village v1 @origin <= 800\nvillage v1 @origin <= 800")).toThrow(DslError);
  });

  it("parses a biome-presence probe", () => {
    const steps = parse("biome swamp1 @origin <= 1000");
    expect(steps[0]).toMatchObject({ structure: "biome", name: "swamp1", min: 0, max: 1000 });
  });

  it("accepts attached and separate @ anchors", () => {
    expect(parse("village v1 @origin <= 800")[0]!.anchor).toBe("origin");
    expect(parse("village v1 @ origin <= 800")[0]!.anchor).toBe("origin");
  });
});

describe("serialize", () => {
  it("round-trips stably", () => {
    expect(roundTrip(EXAMPLE)).toBe(roundTrip(roundTrip(EXAMPLE)));
  });

  it("re-parses its own output to identical steps", () => {
    const steps = parse(EXAMPLE);
    const text = serialize(steps);
    expect(parse(text)).toEqual(steps);
  });

  it("formats ranges per the Rust serializer", () => {
    expect(serialize([{ name: "v1", structure: "village", anchor: "origin", min: 0, max: 800, biomeGate: [] }])).toBe(
      "village v1 @ origin <= 800",
    );
    expect(serialize([{ name: "x1", structure: "mansion", anchor: "origin", min: 3000, max: MAX_DIST, biomeGate: [] }])).toBe(
      "mansion x1 @ origin >= 3000",
    );
    expect(
      serialize([{ name: "t1", structure: "desert_pyramid", anchor: "v1", min: 600, max: 1200, biomeGate: ["desert"] }]),
    ).toBe("desert_pyramid t1 @ v1 in 600..1200; biome=desert");
  });
});
