import { expect, test } from "vitest";
import { matchShortcut } from "./shortcuts";

const key = (k: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {}) => ({
  key: k,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
  metaKey: mods.meta ?? false,
});

test.each([
  ["Ctrl+W", key("w", { ctrl: true })],
  ["Ctrl+K", key("k", { ctrl: true })],
  ["Ctrl+R", key("r", { ctrl: true })],
  ["Ctrl+T", key("t", { ctrl: true })],
  ["Ctrl+C", key("c", { ctrl: true })],
  ["Ctrl+D", key("d", { ctrl: true })],
  ["Ctrl+L", key("l", { ctrl: true })],
  ["Ctrl+Shift+C", key("C", { ctrl: true, shift: true })],
  ["Ctrl+Shift+V", key("V", { ctrl: true, shift: true })],
  ["Alt+B", key("b", { alt: true })],
  ["Alt+F", key("f", { alt: true })],
  ["plain letter", key("a")],
  ["Tab", key("Tab")],
])("%s reaches the shell untouched", (_name, event) => {
  expect(matchShortcut(event, "terminal")).toBeNull();
});

test.each([
  ["Ctrl+Shift+T", key("T", { ctrl: true, shift: true }), "newTab"],
  ["Ctrl+Shift+t", key("t", { ctrl: true, shift: true }), "newTab"],
  ["Ctrl+Shift+W", key("W", { ctrl: true, shift: true }), "closeTab"],
  ["Ctrl+Shift+P", key("P", { ctrl: true, shift: true }), "palette"],
  ["Ctrl+Tab", key("Tab", { ctrl: true }), "nextTab"],
  ["Ctrl+PageDown", key("PageDown", { ctrl: true }), "nextTab"],
  ["Ctrl+Shift+Tab", key("Tab", { ctrl: true, shift: true }), "prevTab"],
  ["Ctrl+PageUp", key("PageUp", { ctrl: true }), "prevTab"],
])("%s is %s in the terminal", (_name, event, action) => {
  expect(matchShortcut(event, "terminal")).toBe(action);
});

test("Ctrl+K opens the palette outside the terminal only", () => {
  expect(matchShortcut(key("k", { ctrl: true }), "app")).toBe("palette");
  expect(matchShortcut(key("k", { ctrl: true }), "terminal")).toBeNull();
});

test("extra modifiers do not trigger a shortcut", () => {
  expect(matchShortcut(key("T", { ctrl: true, shift: true, alt: true }), "app")).toBeNull();
  expect(matchShortcut(key("k", { ctrl: true, meta: true }), "app")).toBeNull();
});
