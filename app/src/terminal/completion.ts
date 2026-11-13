// Suggestions while typing: history, snippets, paths and known commands, in that order. Only
// prefixes are offered, so taking one just adds the missing end; a snippet found by its name
// replaces the line (Ctrl+U first).
import { KNOWN_COMMANDS, PREFIX_COMMANDS, type CommandSpec } from "./commands";

export type Suggestion = {
  label: string;
  insert: string;
  replace: boolean;
  source: "history" | "snippet" | "path" | "command";
};

export type Sources = {
  /** Most recent first. */
  history: string[];
  snippets: { name: string; content: string }[];
  /** Names in the directory of the current word, or `null` when there is none. */
  paths: string[] | null;
};

export const MAX_SUGGESTIONS = 8;
const MIN_LINE = 2;

export function currentWord(line: string): { word: string; start: number } {
  const start = line.lastIndexOf(" ") + 1;
  return { word: line.slice(start), start };
}

/** The directory to list for a word that looks like a path, and the part of the name typed. */
export function pathQuery(word: string, cwd: string | null): { dir: string; prefix: string } | null {
  const looksLikePath = word.startsWith("/") || word.startsWith("~") || word.startsWith("./") || word.startsWith("../") || word.includes("/");
  if (!looksLikePath) return null;
  const cut = word.lastIndexOf("/");
  if (cut < 0) return word === "~" ? { dir: "~/", prefix: "" } : null;
  const dir = word.slice(0, cut + 1);
  const prefix = word.slice(cut + 1);
  if (dir.startsWith("/") || dir.startsWith("~")) return { dir, prefix };
  if (!cwd) return null;
  return { dir: `${cwd.endsWith("/") ? cwd : `${cwd}/`}${dir}`, prefix };
}

/** Snippet text as typed: line ends as Enter, without running the last line. */
function pasted(content: string): string {
  return content.replace(/\r?\n/g, "\r").replace(/\r+$/, "");
}

/** What taking `s` sends to the shell. */
export function insertText(s: Suggestion): string {
  return s.replace ? `\x15${s.insert}` : s.insert;
}

/** The spec reached by the finished words of the line, past `sudo` and the like. */
function specFor(words: string[]): CommandSpec | null {
  let i = 0;
  while (i < words.length && PREFIX_COMMANDS.has(words[i])) i += 1;
  const first = words[i];
  if (first === undefined) return null;
  let spec: CommandSpec | undefined = KNOWN_COMMANDS[first];
  for (const word of words.slice(i + 1)) {
    if (!spec) return null;
    if (word.startsWith("-")) continue;
    spec = spec.sub?.[word] ?? spec;
  }
  return spec ?? null;
}

function commandSuggestions(line: string): Suggestion[] {
  const { word, start } = currentWord(line);
  const done = line.slice(0, start).trim().split(/\s+/).filter(Boolean);
  const onlyPrefixes = done.every((w) => PREFIX_COMMANDS.has(w));
  let names: string[];
  if (onlyPrefixes) {
    names = Object.keys(KNOWN_COMMANDS);
  } else {
    const spec = specFor(done);
    if (!spec) return [];
    names = word.startsWith("-") ? (spec.flags ?? []) : [...Object.keys(spec.sub ?? {}), ...(spec.flags ?? [])];
  }
  if (word === "") return [];
  return names
    .filter((name) => name.startsWith(word) && name !== word)
    .sort()
    .map((name) => ({ label: name, insert: name.slice(word.length), replace: false, source: "command" as const }));
}

export function suggest(line: string, sources: Sources): Suggestion[] {
  if (line.trim().length < MIN_LINE) return [];
  const out: Suggestion[] = [];
  const seen = new Set<string>();
  const add = (s: Suggestion) => {
    // Two suggestions that end in the same line are one.
    const result = s.replace ? s.insert : line + s.insert;
    if (seen.has(result) || out.length >= MAX_SUGGESTIONS) return;
    seen.add(result);
    out.push(s);
  };
  for (const command of sources.history) {
    if (command.startsWith(line) && command !== line) {
      add({ label: command, insert: command.slice(line.length), replace: false, source: "history" });
    }
  }
  const query = line.trim().toLocaleLowerCase();
  for (const snippet of sources.snippets) {
    const content = pasted(snippet.content);
    if (content.startsWith(line) && content !== line) {
      add({ label: snippet.name, insert: content.slice(line.length), replace: false, source: "snippet" });
    } else if (snippet.name.toLocaleLowerCase().includes(query)) {
      add({ label: snippet.name, insert: content, replace: true, source: "snippet" });
    }
  }
  if (sources.paths) {
    const prefix = currentWord(line).word.split("/").pop() ?? "";
    for (const name of sources.paths) {
      if (name.startsWith(prefix) && name !== prefix) {
        add({ label: name, insert: name.slice(prefix.length), replace: false, source: "path" });
      }
    }
  }
  for (const s of commandSuggestions(line)) add(s);
  return out;
}

export type KeyAction = "pass" | "select" | "next" | "prev" | "accept" | "close";

/**
 * What a key does while suggestions may be showing. Nothing is taken from the shell until a
 * suggestion is selected: only Down selects, only Escape closes an unselected list.
 */
export function keyAction(state: { open: boolean; selected: number | null }, key: string): KeyAction {
  if (!state.open) return "pass";
  if (state.selected === null) {
    if (key === "ArrowDown") return "select";
    if (key === "Escape") return "close";
    return "pass";
  }
  switch (key) {
    case "ArrowDown":
      return "next";
    case "ArrowUp":
      return "prev";
    case "Tab":
    case "Enter":
      return "accept";
    case "Escape":
      return "close";
    default:
      return "pass";
  }
}
