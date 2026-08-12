// clipboard — writing text (the seed) to the system clipboard, with a fallback for
// contexts where the async Clipboard API is unavailable (e.g. non-secure origins).
//
// The copy is a plain async write; the only thing callers need is whether it
// succeeded, so we can surface a "copied" notification honestly (no false positives).

export type ClipboardWriter = (text: string) => Promise<void>;

/** Preferred path: the async Clipboard API (requires a secure context, e.g. localhost). */
export const navigatorClipboardWriter: ClipboardWriter = async (text) => {
  const nav = globalThis.navigator as Navigator | undefined;
  if (nav?.clipboard?.writeText) {
    await nav.clipboard.writeText(text);
    return;
  }
  await legacyClipboardWriter(text);
};

/**
 * Fallback for older/non-secure contexts: a hidden textarea + `execCommand("copy")`.
 * Rejects when the DOM isn't available or the copy fails, so callers can report
 * the failure rather than pretending success.
 */
export function legacyClipboardWriter(text: string): Promise<void> {
  const doc = globalThis.document as Document | undefined;
  if (!doc || typeof doc.execCommand !== "function") {
    return Promise.reject(new Error("Clipboard API unavailable"));
  }
  const textarea = doc.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  doc.body.appendChild(textarea);
  textarea.select();
  let ok = false;
  try {
    ok = doc.execCommand("copy");
  } finally {
    doc.body.removeChild(textarea);
  }
  return ok ? Promise.resolve() : Promise.reject(new Error("execCommand('copy') failed"));
}

/** Copy `text` to the clipboard, returning whether it succeeded. */
export async function copyText(text: string, write: ClipboardWriter = navigatorClipboardWriter): Promise<boolean> {
  try {
    await write(text);
    return true;
  } catch {
    return false;
  }
}
