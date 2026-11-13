import { expect, test } from "vitest";
import { currentWord, insertText, keyAction, keyGate, pathQuery, snippetSource, suggest } from "./completion";

const none = { history: [] as string[], snippets: [] as { name: string; content: string }[], paths: null };
const labels = (line: string, sources = none) => suggest(line, sources).map((s) => s.label);

test("history comes before known commands", () => {
  const s = suggest("gi", { ...none, history: ["git status", "ls"] });
  expect(s[0]).toMatchObject({ label: "git status", insert: "t status", replace: false, source: "history" });
  expect(s.some((x) => x.source === "command" && x.label === "git")).toBe(true);
});

test("subcommands and flags of known commands", () => {
  expect(labels("git ch")).toContain("checkout");
  expect(labels("systemctl re")).toEqual(expect.arrayContaining(["restart", "reload"]));
  expect(labels("sudo systemctl sta")).toEqual(expect.arrayContaining(["start", "status"]));
  expect(labels("ls --al")).toContain("--all");
  expect(suggest("git ch", none).find((s) => s.label === "checkout")?.insert).toBe("eckout");
});

test("a snippet is found by its name and replaces the line", () => {
  const s = suggest("upd", { ...none, snippets: [{ name: "System updaten", content: "sudo apt update && sudo apt upgrade -y" }] });
  expect(s[0]).toMatchObject({ label: "System updaten", replace: true, source: "snippet" });
  expect(insertText(s[0], 3)).toBe("\x7f\x7f\x7fsudo apt update && sudo apt upgrade -y");
});

test("a snippet whose content starts with the line only adds the rest", () => {
  const s = suggest("docker comp", { ...none, snippets: [{ name: "Logs", content: "docker compose logs -f" }] });
  expect(s[0]).toMatchObject({ insert: "ose logs -f", replace: false, source: "snippet" });
});

test("snippet sources are prepared once: line ends as Enter, no Enter at the end", () => {
  expect(snippetSource({ name: "Deploy", content: "cd /srv\r\ngit pull\n" })).toEqual({
    name: "Deploy",
    content: "cd /srv\rgit pull",
  });
});

test("paths of the current word", () => {
  const s = suggest("cat /etc/ap", { ...none, paths: ["apache2/", "apt/", "hosts"] });
  expect(s.map((x) => x.label)).toEqual(["apache2/", "apt/"]);
  expect(s[0]).toMatchObject({ insert: "ache2/", source: "path" });
});

test("the same text shows up once, at most 8, nothing under 2 characters", () => {
  const s = suggest("git st", { ...none, history: ["git status", "git status"], snippets: [{ name: "x", content: "git status" }] });
  expect(s.filter((x) => x.label === "git status")).toHaveLength(1);
  expect(suggest("s", { ...none, history: ["sudo ls"] })).toEqual([]);
  const many = Array.from({ length: 20 }, (_, i) => `echo ${i}`);
  expect(suggest("ec", { ...none, history: many })).toHaveLength(8);
});

test("current word and path queries", () => {
  expect(currentWord("cat /etc/ap")).toEqual({ word: "/etc/ap", start: 4 });
  expect(currentWord("ls ")).toEqual({ word: "", start: 3 });
  expect(pathQuery("/etc/ap", null)).toEqual({ dir: "/etc/", prefix: "ap" });
  expect(pathQuery("~/", null)).toEqual({ dir: "~/", prefix: "" });
  expect(pathQuery("./x", "/srv")).toEqual({ dir: "/srv/./", prefix: "x" });
  expect(pathQuery("./x", null)).toBeNull();
  expect(pathQuery("abc", "/srv")).toBeNull();
  expect(pathQuery("src/ma", "/home/me/")).toEqual({ dir: "/home/me/src/", prefix: "ma" });
});

test("insert text adds the rest, or erases the typed line with Backspace first", () => {
  expect(insertText({ label: "git status", insert: "t status", replace: false, source: "history" }, 2)).toBe("t status");
  expect(insertText({ label: "x", insert: "uptime", replace: true, source: "snippet" }, 2)).toBe("\x7f\x7fuptime");
});

function keyEvent(type: string, key: string) {
  return { type, key, prevented: false, stopped: false, preventDefault() { this.prevented = true; }, stopPropagation() { this.stopped = true; } };
}

test("a consumed key is cancelled, and so is the keypress that follows it", () => {
  const gate = keyGate((e) => e.key === "Enter");
  const down = keyEvent("keydown", "Enter");
  expect(gate(down)).toBe(false);
  expect(down.prevented && down.stopped).toBe(true);
  const press = keyEvent("keypress", "Enter");
  expect(gate(press)).toBe(false);
  expect(press.prevented).toBe(true);
  // Only that one keypress.
  expect(gate(keyEvent("keypress", "a"))).toBe(true);
  const other = keyEvent("keydown", "a");
  expect(gate(other)).toBe(true);
  expect(other.prevented).toBe(false);
  expect(gate(keyEvent("keypress", "a"))).toBe(true);
});

test.each([
  [false, null, "ArrowDown", "pass"],
  [true, null, "ArrowUp", "pass"],
  [true, null, "Tab", "pass"],
  [true, null, "Enter", "pass"],
  [true, null, "ArrowDown", "select"],
  [true, null, "Escape", "close"],
  [true, 0, "ArrowDown", "next"],
  [true, 1, "ArrowUp", "prev"],
  [true, 0, "Tab", "accept"],
  [true, 0, "Enter", "accept"],
  [true, 0, "Escape", "close"],
  [true, 0, "a", "pass"],
] as const)("open %s, selected %s, key %s → %s", (open, selected, key, action) => {
  expect(keyAction({ open, selected }, key)).toBe(action);
});
