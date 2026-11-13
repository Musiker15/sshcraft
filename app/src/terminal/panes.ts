// Text that reaches a pane from outside its keyboard (sidebar, run dialog, broadcast) goes
// through the pane itself, so its typed line stays right and a paste is a paste.

export type PaneInput = {
  /** Keys sent to this pane's shell by someone else: the typed line follows them. */
  track(data: string): void;
  /** Pastes `content` into the shell (bracketed where the shell allows), with Enter if `run`. */
  paste(content: string, run: boolean): void;
};

const panes = new Map<string, PaneInput>();

/** Registers a mounted pane; the returned function removes it again. */
export function registerPane(id: string, input: PaneInput): () => void {
  panes.set(id, input);
  return () => {
    if (panes.get(id) === input) panes.delete(id);
  };
}

export function paneInput(id: string): PaneInput | undefined {
  return panes.get(id);
}

/** Sent on `window` when the history was cleared (`detail`: the scope, or null for all). */
export const HISTORY_CLEARED = "sshcraft:history-cleared";
/** Sent on `window` when a snippet was saved or deleted. */
export const SNIPPETS_CHANGED = "sshcraft:snippets-changed";

/** Spaces for a tab where a paste is typed key by key: a tab would start the shell's completion. */
const TAB_AS_SPACES = "    ";

/**
 * The keys for pasting `content`. With bracketed paste the shell inserts all lines and runs
 * nothing until Enter; without it line ends are Enter and each line runs as it arrives.
 */
export function pasteBytes(content: string, run: boolean, bracketed: boolean): string {
  const lines = content.replace(/\r?\n/g, "\r").replace(/\r+$/, "");
  const body = bracketed ? `\x1b[200~${lines}\x1b[201~` : lines.replace(/\t/g, TAB_AS_SPACES);
  return run ? `${body}\r` : body;
}
