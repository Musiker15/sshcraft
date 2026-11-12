// Pure helpers for listing hosts: search, the "user@address" line and sorting.
import type { Group, Host } from "../api";
import type { Language } from "../i18n/language";

/** Composed form and lower case, so "MÜNCHEN", "münchen" and a decomposed "München" match. */
export function normalize(s: string): string {
  return s.normalize("NFC").toLocaleLowerCase();
}

function text(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function effective(host: Host, group: Group | undefined) {
  const own = host.settings;
  const inherited = group?.defaults;
  return {
    address: text(own.address) ?? text(inherited?.address),
    username: text(own.username) ?? text(inherited?.username),
    port: own.port ?? inherited?.port ?? 22,
  };
}

export function filterHosts(hosts: Host[], groups: Group[], query: string): Host[] {
  const q = normalize(query.trim());
  if (!q) return hosts;
  const byId = new Map(groups.map((g) => [g.id, g]));
  return hosts.filter((host) => {
    const group = host.group_id ? byId.get(host.group_id) : undefined;
    const { address, username } = effective(host, group);
    return [host.name, address, username, group?.name].some((field) => field && normalize(field).includes(q));
  });
}

/** `user@address`, with the port only when it is not 22; `null` without an address. */
export function hostSubtitle(host: Host, group: Group | undefined): string | null {
  const { address, username, port } = effective(host, group);
  if (!address) return null;
  return `${username ? `${username}@` : ""}${address}${port === 22 ? "" : `:${port}`}`;
}

export function sortByName<T extends { name: string }>(items: T[], lang: Language): T[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name, lang, { sensitivity: "base" }) || a.name.localeCompare(b.name, lang));
}
