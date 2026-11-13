// The line being typed in a terminal, as far as SSHCraft can know it from the keys sent. Shell
// editing it cannot follow (Tab completion, history, moving the cursor) makes the line unknown
// until the next Enter: then nothing is suggested or recorded for it.

export type Line = { text: string; certain: boolean };

export const emptyLine: Line = { text: "", certain: true };

/** Takes the keys `data` sent to the shell; `committed` is the command that Enter ran, if known. */
export function track(line: Line, data: string): { line: Line; committed: string | null } {
  // Several lines at once (a paste): none of them is recorded.
  const breaks = data.split("\r").length - 1;
  if (breaks > 1 || (breaks === 1 && !data.endsWith("\r"))) {
    return { line: { text: "", certain: data.endsWith("\r") }, committed: null };
  }
  // Arrows, function keys, Alt+…: the shell moves or changes the line in ways not followed.
  if (data.startsWith("\x1b")) {
    return { line: { text: line.text, certain: false }, committed: null };
  }
  let { text, certain } = line;
  let committed: string | null = null;
  for (const ch of data) {
    if (ch === "\r") {
      committed = certain && text.trim() !== "" ? text : null;
      text = "";
      certain = true;
    } else if (ch === "\x7f" || ch === "\b") {
      text = [...text].slice(0, -1).join("");
    } else if (ch === "\x15" || ch === "\x03") {
      // Ctrl+U clears the line, Ctrl+C drops it: either way a fresh, known line.
      text = "";
      certain = true;
    } else if (ch < " ") {
      certain = false;
    } else {
      text += ch;
    }
  }
  return { line: { text, certain }, committed };
}

export type RecordContext = {
  /** The last output asked for a password or passphrase. */
  secretPrompt: boolean;
  /** A full-screen program (vim, less) has the terminal. */
  alternate: boolean;
  /** The keys went to several panes at once. */
  broadcast: boolean;
};

/** Whether a finished command goes into the history. */
export function shouldRecord(command: string, context: RecordContext): boolean {
  if (command.trim() === "" || command.startsWith(" ")) return false;
  return !context.secretPrompt && !context.alternate && !context.broadcast;
}
