// Running a snippet: what is typed, which new tabs open, which open panes are picked first.
import type { Host } from "../api";
import { MAX_PANES, isPicker, type SessionTab } from "../state/tabs";

/** The snippet as keys: line ends as Enter; "run" also presses Enter after the last line. */
export function snippetInput(content: string, run: boolean): string {
  const lines = content.replace(/\r?\n/g, "\r").replace(/\r+$/, "");
  return run ? `${lines}\r` : lines;
}

/** Hosts to connect to, in tabs of at most 16 panes; ids of hosts that are gone are counted. */
export function planNewTabs(hosts: Host[], targetIds: string[]): { tabs: string[][]; missing: number } {
  const known = new Set(hosts.map((h) => h.id));
  const ids = targetIds.filter((id) => known.has(id));
  const tabs: string[][] = [];
  for (let i = 0; i < ids.length; i += MAX_PANES) tabs.push(ids.slice(i, i + MAX_PANES));
  return { tabs, missing: targetIds.length - ids.length };
}

/** Open panes with a session that are connected to one of the snippet's default hosts. */
export function defaultPanes(tabs: SessionTab[], defaultTargets: string[]): string[] {
  const wanted = new Set(defaultTargets);
  return tabs.flatMap((tab) =>
    tab.panes.flatMap((pane) =>
      !isPicker(pane) && pane.sessionId && pane.target.kind === "host" && wanted.has(pane.target.host_id)
        ? [pane.id]
        : [],
    ),
  );
}
