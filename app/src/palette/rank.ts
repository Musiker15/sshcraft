// Ordering for the command palette: hosts first (the common case is "connect to X"), then actions.
import type { Group, Host } from "../api";
import { normalize } from "../pages/hostsView";

export type PaletteActionId =
  | "newHost"
  | "newGroup"
  | "importSshConfig"
  | "openHosts"
  | "openKeys"
  | "openSettings"
  | "newTab"
  | "closeTab";

export type PaletteAction = { id: PaletteActionId; label: string };
export type PaletteItem = { kind: "host"; host: Host } | ({ kind: "action" } & PaletteAction);

const LIMIT = 50;
const RECENT_ON_EMPTY = 5;
const WORD_START = /[\s\-_.]/;

/** 0 = name starts with the query, 1 = a word in the name does, 2 = name contains it,
 *  3 = address or user contains it; null = no match. */
function level(host: Host, group: Group | undefined, q: string): number | null {
  const name = normalize(host.name);
  if (name.startsWith(q)) return 0;
  const at = name.indexOf(q);
  if (at > 0) {
    for (let i = at; i !== -1; i = name.indexOf(q, i + 1)) if (WORD_START.test(name[i - 1])) return 1;
    return 2;
  }
  const address = host.settings.address ?? group?.defaults.address ?? "";
  const username = host.settings.username ?? group?.defaults.username ?? "";
  return normalize(address).includes(q) || normalize(username).includes(q) ? 3 : null;
}

export function rankPalette(
  query: string,
  hosts: Host[],
  groups: Group[],
  recent: string[],
  actions: PaletteAction[],
): PaletteItem[] {
  const q = normalize(query.trim());
  const asAction = (a: PaletteAction): PaletteItem => ({ kind: "action", ...a });
  if (!q) {
    const byId = new Map(hosts.map((h) => [h.id, h]));
    const recentHosts = recent.flatMap((id) => byId.get(id) ?? []).slice(0, RECENT_ON_EMPTY);
    return [...recentHosts.map((host): PaletteItem => ({ kind: "host", host })), ...actions.map(asAction)].slice(0, LIMIT);
  }
  const groupsById = new Map(groups.map((g) => [g.id, g]));
  const recency = new Map(recent.map((id, index) => [id, index]));
  const matches = hosts.flatMap((host) => {
    const l = level(host, host.group_id ? groupsById.get(host.group_id) : undefined, q);
    return l === null ? [] : [{ host, l, r: recency.get(host.id) ?? Number.MAX_SAFE_INTEGER }];
  });
  matches.sort((a, b) => a.l - b.l || a.r - b.r || a.host.name.localeCompare(b.host.name));
  const matchingActions = actions.filter((a) => normalize(a.label).includes(q));
  return [...matches.map(({ host }): PaletteItem => ({ kind: "host", host })), ...matchingActions.map(asAction)].slice(
    0,
    LIMIT,
  );
}
