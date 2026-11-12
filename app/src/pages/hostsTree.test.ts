import { expect, test } from "vitest";
import type { Group, Host } from "../api";
import { allTags, chain, childGroups, descendants, groupLabel, hasTags, inheritedDefaults } from "./hostsTree";
import { filterHosts, hostSubtitle } from "./hostsView";

const empty = { address: null, port: null, username: null, key: null };
const groups: Group[] = [
  { id: "top", name: "Kunden", defaults: { ...empty, address: "kunden.example.org" }, parent_id: null, tags: [] },
  { id: "mid", name: "Müller", defaults: { ...empty, username: "deploy" }, parent_id: "top", tags: ["vip"] },
  { id: "low", name: "Web", defaults: empty, parent_id: "mid", tags: [] },
  { id: "other", name: "Privat", defaults: empty, parent_id: null, tags: [] },
];
const host = (id: string, group: string | null, tags: string[] = []): Host => ({
  id,
  name: id,
  group_id: group,
  settings: empty,
  tags,
});

test("chain and path of a group", () => {
  expect(chain(groups, "low").map((g) => g.id)).toEqual(["low", "mid", "top"]);
  expect(groupLabel(groups, "low")).toBe("Kunden / Müller / Web");
  expect(chain(groups, null)).toEqual([]);
});

test("a cycle does not hang", () => {
  const looped: Group[] = [
    { id: "a", name: "a", defaults: empty, parent_id: "b", tags: [] },
    { id: "b", name: "b", defaults: empty, parent_id: "a", tags: [] },
  ];
  expect(chain(looped, "a").length).toBe(2);
});

test("hosts of one level and subgroups", () => {
  expect(childGroups(groups, null).map((g) => g.id)).toEqual(["top", "other"]);
  expect(childGroups(groups, "top").map((g) => g.id)).toEqual(["mid"]);
  expect([...descendants(groups, "top")].sort()).toEqual(["low", "mid"]);
});

test("inheritance over all levels", () => {
  const d = inheritedDefaults(groups, "low");
  expect(d.address).toBe("kunden.example.org");
  expect(d.username).toBe("deploy");
  expect(hostSubtitle(host("h", "low"), groups)).toBe("deploy@kunden.example.org");
});

test("search over all levels finds paths and tags", () => {
  const hosts = [host("a", "low", ["prod"]), host("b", "other")];
  expect(filterHosts(hosts, groups, "müller").map((h) => h.id)).toEqual(["a"]);
  expect(filterHosts(hosts, groups, "prod").map((h) => h.id)).toEqual(["a"]);
});

test("tag filter needs every chosen tag", () => {
  const h = host("a", null, ["Prod", "web"]);
  expect(hasTags(h, ["prod"])).toBe(true);
  expect(hasTags(h, ["prod", "web"])).toBe(true);
  expect(hasTags(h, ["prod", "db"])).toBe(false);
  expect(allTags([h, host("b", null, ["db", "PROD"])])).toEqual(["db", "Prod", "web"]);
});
