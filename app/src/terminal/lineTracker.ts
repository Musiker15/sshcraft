// The line being typed in a terminal, as far as SSHCraft can know it from the keys sent. Shell
// editing it cannot follow (Tab completion, history, moving the cursor) makes the line unknown
// until the next Enter: then nothing is suggested or recorded for it.
//
// What kind of line it is gets decided from the prompt when its first key is typed, and is kept
// until Enter: a password typed after "Password:" stays secret even when the server prints an
// asterisk per key.

/** "shell": at a shell prompt; "secret": after a password-like question; "other": anything else
 * (mysql, psql, python, a prompt SSHCraft does not recognise). */
export type LineKind = "shell" | "secret" | "other";

export type Line = { text: string; certain: boolean; kind: LineKind | null };

export const emptyLine: Line = { text: "", certain: true, kind: null };

// eslint-disable-next-line no-control-regex
const ANSI = /\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])/g;
const SECRET =
  /(password|passwort|kennwort|passphrase|pass phrase|mot de passe|contrase[ñn]a|senha|parola|token|verification code|one-time|otp|2fa|\bpin\b)[^\n]*[:?]\s*\**\s*$/i;
/** Prompts of programs that read lines but are not a shell: their input is not kept. */
const NOT_A_SHELL = [
  /^(mysql|mariadb \[.*\]|sqlite|redis[^>]*|mongo[^>]*)>\s*$/i,
  /=[#>-]\s*$/, // psql
  /^(>>>|\.\.\.)\s*$/, // python
  /^irb.*>\s*$/,
  /^In \[\d+\]:\s*$/, // ipython
  /^>\s*$/, // a shell's continuation prompt
];
const SHELL_END = /[$#%>❯➜λ»]\s*$/;

/** What the last line of output asks for. */
export function classifyPrompt(tail: string): LineKind {
  const lines = tail.replace(ANSI, "").split(/\r?\n|\r/);
  const last = lines[lines.length - 1];
  if (SECRET.test(last)) return "secret";
  const trimmed = last.trim();
  if (NOT_A_SHELL.some((r) => r.test(trimmed))) return "other";
  return SHELL_END.test(last) ? "shell" : "other";
}

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

/**
 * Takes the keys `data` sent to the shell; `committed` is the command that Enter ran, if known.
 * `prompt` is the output just before the keys, used when they start a line.
 */
export function track(line: Line, data: string, prompt = ""): { line: Line; committed: string | null } {
  const kind = line.kind ?? (line.text === "" ? classifyPrompt(prompt) : "other");
  // A bracketed paste is plain text inserted at the cursor.
  if (data.startsWith(PASTE_START) && data.endsWith(PASTE_END)) {
    const text = data.slice(PASTE_START.length, -PASTE_END.length);
    if (/[\r\n]/.test(text) || /[\x00-\x1f\x7f]/.test(text.replace(/\t/g, ""))) {
      return { line: { text: line.text, certain: false, kind }, committed: null };
    }
    return { line: { text: line.text + text, certain: line.certain, kind }, committed: null };
  }
  // Several lines at once (a paste): none of them is recorded.
  const breaks = data.split("\r").length - 1;
  if (breaks > 1 || (breaks === 1 && !data.endsWith("\r"))) {
    return { line: { text: "", certain: data.endsWith("\r"), kind: data.endsWith("\r") ? null : kind }, committed: null };
  }
  // Arrows, function keys, Alt+…: the shell moves or changes the line in ways not followed.
  if (data.startsWith("\x1b")) {
    return { line: { text: line.text, certain: false, kind }, committed: null };
  }
  let { text, certain } = line;
  let current: LineKind | null = kind;
  let committed: string | null = null;
  for (const ch of data) {
    if (ch === "\r") {
      committed = certain && text.trim() !== "" ? text : null;
      text = "";
      certain = true;
      current = null;
    } else if (ch === "\x7f" || ch === "\b") {
      text = [...text].slice(0, -1).join("");
    } else if (ch === "\x15" || ch === "\x03") {
      // Ctrl+U clears the line, Ctrl+C drops it: either way a fresh, known line.
      text = "";
      certain = true;
    } else if (ch < " ") {
      certain = false;
    } else {
      current ??= classifyPrompt(prompt);
      text += ch;
    }
  }
  return { line: { text, certain, kind: current }, committed };
}

export type RecordContext = {
  /** What kind of line it was. */
  kind: LineKind | null;
  /** A full-screen program (vim, less) has the terminal. */
  alternate: boolean;
  /** The keys went to several panes at once. */
  broadcast: boolean;
};

/** Whether a finished command goes into the history: only lines typed at a shell prompt. */
export function shouldRecord(command: string, context: RecordContext): boolean {
  if (command.trim() === "" || command.startsWith(" ")) return false;
  return context.kind === "shell" && !context.alternate && !context.broadcast;
}
