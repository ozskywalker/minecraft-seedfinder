import { describe, expect, it } from "vitest";
import { copyText } from "./clipboard";

describe("copyText", () => {
  it("writes the seed via the injected writer and reports success", async () => {
    let received = "";
    const writer = async (t: string) => {
      received = t;
    };
    await expect(copyText("12345", writer)).resolves.toBe(true);
    expect(received).toBe("12345");
  });

  it("reports failure when the writer rejects (e.g. permission denied)", async () => {
    const writer = async () => {
      throw new Error("denied");
    };
    await expect(copyText("12345", writer)).resolves.toBe(false);
  });

  it("reports failure, never success, when no clipboard is available", async () => {
    // Node's test environment has neither navigator.clipboard nor a DOM to fall
    // back on, so the default writer cannot copy — the result must be false, not a
    // fabricated success.
    await expect(copyText("12345")).resolves.toBe(false);
  });

  it("copies the empty seed string as-is", async () => {
    let received = "sentinel";
    const writer = async (t: string) => {
      received = t;
    };
    await expect(copyText("", writer)).resolves.toBe(true);
    expect(received).toBe("");
  });
});
