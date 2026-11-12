import { expect, test } from "vitest";
import type { Host } from "../api";
import { rankPalette, type PaletteItem } from "./rank";

const host = (id: string, name: string, address: string | null = null, username: string | null = null): Host => ({
  id,
  name,
  group_id: null,
  settings: { address, port: null, username, key: null },
});
const actions = [
  { id: "newHost" as const, label: "New host" },
  { id: "openSettings" as const, label: "Open settings" },
];
const names = (items: PaletteItem[]) => items.map((i) => (i.kind === "host" ? i.host.name : `action:${i.id}`));

test("empty search lists recent hosts in their order, at most five, then actions", () => {
  const hosts = ["a", "b", "c", "d", "e", "f", "g"].map((n) => host(n, n));
  const recent = ["c", "gone", "a", "b", "d", "e", "f", "g"];
  expect(names(rankPalette("", hosts, [], recent, actions))).toEqual([
    "c", "a", "b", "d", "e", "action:newHost", "action:openSettings",
  ]);
});

test("name prefix beats word prefix beats substring", () => {
  const hosts = [host("1", "prodweb"), host("2", "my-web"), host("3", "web-01")];
  expect(names(rankPalette("web", hosts, [], [], actions))).toEqual(["web-01", "my-web", "prodweb"]);
});

test("address and user are searched too", () => {
  const hosts = [host("1", "db", "10.0.0.7"), host("2", "app", "app.example.org", "deploy")];
  expect(names(rankPalette("10.0.0", hosts, [], [], actions))).toEqual(["db"]);
  expect(names(rankPalette("deploy", hosts, [], [], actions))).toEqual(["app"]);
});

test("matching actions come after all hosts", () => {
  const hosts = [host("1", "settings-box")];
  expect(names(rankPalette("set", hosts, [], [], actions))).toEqual(["settings-box", "action:openSettings"]);
});

test("within a level recently used hosts come first", () => {
  const hosts = [host("1", "web-a"), host("2", "web-b")];
  expect(names(rankPalette("web", hosts, [], ["2"], actions))).toEqual(["web-b", "web-a"]);
});

test("at most 50 entries", () => {
  const hosts = Array.from({ length: 80 }, (_, i) => host(String(i), `web-${i}`));
  expect(rankPalette("web", hosts, [], [], actions)).toHaveLength(50);
});

test("umlauts match regardless of case", () => {
  expect(names(rankPalette("MÜNCHEN", [host("1", "münchen")], [], [], actions))).toEqual(["münchen"]);
});
