// Pure helpers for nested groups and tags.
import type { ConnectionSettings, Group } from "../api";

/** As in the Rust core: deeper nesting (or a cycle) is cut off here. */
const MAX_DEPTH = 64;

/** A group and the groups above it, nearest first; stops at a missing group or a cycle. */
export function chain(groups: Group[], id: string | null | undefined): Group[] {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const out: Group[] = [];
  let next = id ?? null;
  while (next && out.length < MAX_DEPTH && !out.some((g) => g.id === next)) {
    const group = byId.get(next);
    if (!group) break;
    out.push(group);
    next = group.parent_id ?? null;
  }
  return out;
}

/** "Top / Middle / Group". */
export function groupLabel(groups: Group[], id: string | null | undefined): string {
  return chain(groups, id)
    .reverse()
    .map((g) => g.name)
    .join(" / ");
}

/** The groups right below `parent` (`null`: the top level). */
export function childGroups(groups: Group[], parent: string | null): Group[] {
  return groups.filter((g) => (g.parent_id ?? null) === parent);
}

/** Every group below `id`, at any depth. */
export function descendants(groups: Group[], id: string): Set<string> {
  const found = new Set<string>();
  let frontier = [id];
  for (let depth = 0; depth < MAX_DEPTH && frontier.length > 0; depth++) {
    const next: string[] = [];
    for (const g of groups) {
      if (g.parent_id && frontier.includes(g.parent_id) && !found.has(g.id) && g.id !== id) {
        found.add(g.id);
        next.push(g.id);
      }
    }
    frontier = next;
  }
  return found;
}

function text(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

/** What a host in group `id` inherits: from the nearest group that sets each value. */
export function inheritedDefaults(groups: Group[], id: string | null | undefined): ConnectionSettings {
  const out: ConnectionSettings = { address: null, port: null, username: null, key: null };
  for (const g of chain(groups, id)) {
    out.address ??= text(g.defaults.address);
    out.port ??= g.defaults.port;
    out.username ??= text(g.defaults.username);
    out.key ??= g.defaults.key;
  }
  return out;
}

const lower = (s: string) => s.normalize("NFC").toLocaleLowerCase();

/** Every tag in use, once (first spelling wins), sorted. */
export function allTags(items: { tags?: string[] }[]): string[] {
  const seen = new Map<string, string>();
  for (const item of items) for (const tag of item.tags ?? []) if (!seen.has(lower(tag))) seen.set(lower(tag), tag);
  return [...seen.values()].sort((a, b) => lower(a).localeCompare(lower(b)));
}

/** Whether an item has every one of `wanted` (ignoring case). */
export function hasTags(item: { tags?: string[] }, wanted: string[]): boolean {
  const own = new Set((item.tags ?? []).map(lower));
  return wanted.every((t) => own.has(lower(t)));
}

/** A host's own tags and those of every group above it. */
export function hostTags(host: { tags?: string[]; group_id: string | null }, groups: Group[]): string[] {
  return [...(host.tags ?? []), ...chain(groups, host.group_id).flatMap((g) => g.tags ?? [])];
}
