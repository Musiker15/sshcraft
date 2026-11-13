// Terminal helpers that decide what output may do: clipboard writes (OSC 52), the reported
// working directory (OSC 7), which links open, when a pane waits for input, and what a template
// types after connecting.
import type { SessionTarget } from "../api";
import type { Startup } from "../state/tabs";

export const MAX_CLIPBOARD = 1024 * 1024;
const MAX_LINK = 2048;

/**
 * The text an OSC 52 sequence (`<selection>;<base64>`) wants on the clipboard. Queries (`?`),
 * empty, broken and oversized payloads give `null`: the clipboard is never read and never cleared.
 */
export function osc52Text(data: string, maxBytes = MAX_CLIPBOARD): string | null {
  const split = data.indexOf(";");
  if (split < 0) return null;
  const payload = data.slice(split + 1);
  if (payload === "" || payload === "?") return null;
  // Checked before decoding, so a huge payload costs nothing.
  if (Math.floor((payload.length * 3) / 4) > maxBytes + 2) return null;
  let binary: string;
  try {
    binary = atob(payload);
  } catch {
    return null;
  }
  if (binary.length > maxBytes) return null;
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

/** The directory an OSC 7 sequence (`file://host/path`) reports. */
export function osc7Path(data: string): string | null {
  let url: URL;
  try {
    url = new URL(data);
  } catch {
    return null;
  }
  if (url.protocol !== "file:") return null;
  let path: string;
  try {
    path = decodeURIComponent(url.pathname);
  } catch {
    // A lone "%" in the path: not a directory we can name.
    return null;
  }
  // file:///C:/Users -> C:/Users
  return /^\/[A-Za-z]:/.test(path) ? path.slice(1) : path;
}

export function isOpenableLink(url: string): boolean {
  if (url.length > MAX_LINK) return false;
  try {
    const parsed = new URL(url);
    return (parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.hostname !== "";
  } catch {
    return false;
  }
}

const ANSI = /\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])/g;
const PROMPTS = [
  /(password|passwort|kennwort)[^\n]*:$/i,
  /passphrase[^\n]*:$/i,
  /\[y\/n\]\s*[:?]?$/i,
  /\(yes\/no(\/\[fingerprint\])?\)\s*\??$/i,
  /continue\?$/i,
];

/** Whether the last line of output asks for something (a password, yes or no). */
export function expectsInput(tail: string): boolean {
  const lines = tail.replace(ANSI, "").split(/\r?\n|\r/);
  const last = lines[lines.length - 1].trim();
  return last !== "" && PROMPTS.some((p) => p.test(last));
}

// Every answer xterm 6 sends on its own (`triggerDataEvent` in its InputHandler), and mouse reports.
const REPORTS = [
  /^\x1b\[\??\d+;\d+R$/, // cursor position (DSR 6, DECXCPR)
  /^\x1b\[\d*n$/, // status report (DSR 5)
  /^\x1b\[[?>]?[\d;]*c$/, // device attributes
  /^\x1b\[[IO]$/, // focus in and out
  /^\x1b\][^\x07\x1b]*(\x07|\x1b\\)$/, // OSC answers (colours and the like)
  /^\x1bP[\s\S]*\x1b\\$/, // DCS answers (DECRQSS)
  /^\x1b\[\??[\d;]*\$y$/, // mode reports
  /^\x1b\[\d+(;\d+)*t$/, // window reports
  /^\x1b\[<\d+;\d+;\d+[Mm]$/, // mouse, SGR encoding
  /^\x1b\[M[\s\S]{3}$/, // mouse, X10 encoding
];

/**
 * Whether `data` from xterm is the terminal answering the server (cursor position, device
 * attributes, focus, DECRQSS) or a mouse report rather than the user typing. A broadcast never
 * sends those to other panes; they go to their own session only.
 */
export function isTerminalReport(data: string): boolean {
  return REPORTS.some((r) => r.test(data));
}

function shellQuote(text: string): string {
  return `'${text.split("'").join("'\\''")}'`;
}

/** A directory for `cd`: a leading `~` or `~/` stays outside the quotes so the shell expands it. */
function quoteDir(dir: string): string {
  if (dir === "~") return "~";
  if (dir.startsWith("~/")) return `~/${shellQuote(dir.slice(2))}`;
  return shellQuote(dir);
}

/**
 * What a template types once after the first connect. Over SSH it changes directory first
 * (POSIX shell); a local shell already starts there, a serial line has none.
 */
export function startupInput(target: SessionTarget, startup: Startup | undefined): string | null {
  const cwd = startup?.cwd?.trim() ?? "";
  const command = startup?.command?.trim() ?? "";
  const parts = [target.kind === "host" && cwd !== "" ? `cd -- ${quoteDir(cwd)}` : "", command].filter(
    (p) => p !== "",
  );
  return parts.length === 0 ? null : `${parts.join(" && ")}\r`;
}
