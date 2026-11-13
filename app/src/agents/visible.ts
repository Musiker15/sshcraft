// Text from an agent shown to the user: characters that hide or reorder text are made visible,
// so a command looks like what will run.

const BIDI = /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/;

function mark(c: string): string {
  return `\u27E8U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}\u27E9`;
}

/** `text` with control characters (but line ends and tabs) and bidi controls shown as ⟨U+XXXX⟩. */
export function visibleText(text: string): string {
  let out = "";
  for (const c of text) {
    const code = c.codePointAt(0)!;
    const control = (code < 0x20 && c !== "\n" && c !== "\t") || (code >= 0x7f && code < 0xa0);
    out += control || BIDI.test(c) ? mark(c) : c;
  }
  return out;
}

/** Whether the text needs a closer look: several lines, very long, or with hidden characters. */
export function needsCareful(text: string): boolean {
  return text.includes("\n") || text.length > 300 || visibleText(text) !== text;
}
