import { expect, test } from "vitest";
import type { ConnectionSettings, Group, Host } from "../api";
import { filterHosts, hostSubtitle, normalize, sortByName } from "./hostsView";

const settings = (s: Partial<ConnectionSettings> = {}): ConnectionSettings => ({
  address: null,
  port: null,
  username: null,
  key: null,
  ...s,
});
const host = (name: string, s: Partial<ConnectionSettings> = {}, group_id: string | null = null): Host => ({
  id: name,
  name,
  group_id,
  settings: settings(s),
});
const group: Group = { id: "g1", name: "Produktion", defaults: settings({ username: "deploy", address: "10.0.0.5", port: 2222 }) };

test("search ignores case, also for umlauts", () => {
  expect(filterHosts([host("münchen")], [], "MÜNCHEN").map((h) => h.name)).toEqual(["münchen"]);
});

test("search finds decomposed umlauts", () => {
  expect(filterHosts([host("München")], [], "münchen")).toHaveLength(1);
  expect(normalize("München")).toBe(normalize("München"));
});

test("search finds names with emoji", () => {
  expect(filterHosts([host("🚀 prod")], [], "prod")).toHaveLength(1);
});

test("search finds by address, own or inherited user, and group name", () => {
  const hosts = [host("a", { address: "web.example.org" }), host("b", {}, "g1"), host("c")];
  expect(filterHosts(hosts, [group], "example").map((h) => h.name)).toEqual(["a"]);
  expect(filterHosts(hosts, [group], "deploy").map((h) => h.name)).toEqual(["b"]);
  expect(filterHosts(hosts, [group], "produktion").map((h) => h.name)).toEqual(["b"]);
});

test("empty search keeps every host", () => {
  expect(filterHosts([host("a"), host("b")], [], "  ")).toHaveLength(2);
});

test("subtitle shows user and address, inherits from the group and hides port 22", () => {
  expect(hostSubtitle(host("a", { address: "example.org", username: "root" }), [])).toBe("root@example.org");
  expect(hostSubtitle(host("a", { address: "example.org", port: 22 }), [])).toBe("example.org");
  expect(hostSubtitle(host("b", {}, "g1"), [group])).toBe("deploy@10.0.0.5:2222");
  expect(hostSubtitle(host("c", { address: "10.0.0.1", username: "root", port: 2222 }), [])).toBe(
    "root@10.0.0.1:2222",
  );
  expect(hostSubtitle(host("d", { username: "root" }), [])).toBeNull();
});

test("sorting follows the language", () => {
  const names = sortByName([{ name: "Birne" }, { name: "Äpfel" }, { name: "Apfel" }], "de").map((x) => x.name);
  expect(names).toEqual(["Apfel", "Äpfel", "Birne"]);
});
