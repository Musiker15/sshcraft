import { expect, test } from "vitest";
import {
  comboOf,
  findConflict,
  isTerminalSafe,
  isValidCombo,
  matchShortcut,
  resolveBindings,
} from "./shortcuts";

const B = resolveBindings({});

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
  ["Ctrl+PageUp", key("PageUp", { ctrl: true })],
  ["Ctrl+PageDown", key("PageDown", { ctrl: true })],
])("%s reaches the shell untouched", (_name, event) => {
  expect(matchShortcut(event, "terminal", B)).toBeNull();
});

test.each([
  ["Ctrl+Shift+T", key("T", { ctrl: true, shift: true }), "newTab"],
  ["Ctrl+Shift+t", key("t", { ctrl: true, shift: true }), "newTab"],
  ["Ctrl+Shift+W", key("W", { ctrl: true, shift: true }), "closeTab"],
  ["Ctrl+Shift+P", key("P", { ctrl: true, shift: true }), "palette"],
  ["Ctrl+Tab", key("Tab", { ctrl: true }), "nextTab"],
  ["Ctrl+Shift+Tab", key("Tab", { ctrl: true, shift: true }), "prevTab"],
])("%s is %s in the terminal", (_name, event, action) => {
  expect(matchShortcut(event, "terminal", B)).toBe(action);
});

test("Ctrl+K opens the palette outside the terminal only", () => {
  expect(matchShortcut(key("k", { ctrl: true }), "app", B)).toBe("palette");
  expect(matchShortcut(key("k", { ctrl: true }), "terminal", B)).toBeNull();
});

test("extra modifiers do not trigger a shortcut", () => {
  expect(matchShortcut(key("T", { ctrl: true, shift: true, alt: true }), "app", B)).toBeNull();
  expect(matchShortcut(key("k", { ctrl: true, meta: true }), "app", B)).toBeNull();
});

test("Ctrl+PageUp and Ctrl+PageDown switch tabs outside the terminal only", () => {
  expect(matchShortcut(key("PageDown", { ctrl: true }), "app", B)).toBe("nextTab");
  expect(matchShortcut(key("PageUp", { ctrl: true }), "app", B)).toBe("prevTab");
});

test("comboOf writes modifiers in a fixed order", () => {
  expect(comboOf(key("p", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+P");
  expect(comboOf(key("F5", { alt: true, shift: true }))).toBe("Alt+Shift+F5");
  expect(comboOf(key("Shift", { shift: true }))).toBeNull();
  expect(comboOf(key("Control", { ctrl: true }))).toBeNull();
});

test("combos are checked like in Rust", () => {
  for (const good of ["Ctrl+Shift+P", "Alt+X", "F5", "Ctrl+Tab", "Meta+K"]) expect(isValidCombo(good)).toBe(true);
  for (const bad of ["P", "Shift+P", "Ctrl+", "Shift+Ctrl+P", "Ctrl+Shift+P+Q", "F25", ""])
    expect(isValidCombo(bad)).toBe(false);
});

test("a custom binding replaces the default", () => {
  const b = resolveBindings({ newTab: ["Ctrl+Shift+N"] });
  expect(matchShortcut(key("N", { ctrl: true, shift: true }), "terminal", b)).toBe("newTab");
  expect(matchShortcut(key("T", { ctrl: true, shift: true }), "terminal", b)).toBeNull();
});

test("an empty list unbinds the action", () => {
  const b = resolveBindings({ palette: [] });
  expect(matchShortcut(key("P", { ctrl: true, shift: true }), "app", b)).toBeNull();
  expect(matchShortcut(key("k", { ctrl: true }), "app", b)).toBeNull();
});

test("a binding that is not terminal-safe works outside the terminal only", () => {
  const b = resolveBindings({ closeTab: ["Ctrl+W"], newTab: ["Ctrl+Alt+Q"] });
  expect(matchShortcut(key("w", { ctrl: true }), "terminal", b)).toBeNull();
  expect(matchShortcut(key("w", { ctrl: true }), "app", b)).toBe("closeTab");
  expect(matchShortcut(key("q", { ctrl: true, alt: true }), "terminal", b)).toBeNull();
  expect(matchShortcut(key("q", { ctrl: true, alt: true }), "app", b)).toBe("newTab");
});

test("terminal-safe means Ctrl+Shift without Alt, or Ctrl+Tab", () => {
  expect(isTerminalSafe("Ctrl+Shift+P")).toBe(true);
  expect(isTerminalSafe("Ctrl+Tab")).toBe(true);
  expect(isTerminalSafe("Ctrl+Shift+Tab")).toBe(true);
  expect(isTerminalSafe("Ctrl+W")).toBe(false);
  expect(isTerminalSafe("Ctrl+Alt+Shift+Q")).toBe(false);
  expect(isTerminalSafe("Alt+Shift+Q")).toBe(false);
  expect(isTerminalSafe("F5")).toBe(false);
});

test("findConflict names the action that already has the combo", () => {
  expect(findConflict(B, "newTab", "Ctrl+Shift+P")).toBe("palette");
  expect(findConflict(B, "palette", "Ctrl+Shift+P")).toBeNull();
  expect(findConflict(B, "newTab", "Ctrl+Shift+N")).toBeNull();
});

test.each([
  ["Ctrl+Shift+D", key("D", { ctrl: true, shift: true }), "splitPane"],
  ["Ctrl+Shift+X", key("X", { ctrl: true, shift: true }), "closePane"],
  ["Ctrl+Shift+B", key("B", { ctrl: true, shift: true }), "toggleBroadcast"],
  ["Ctrl+Shift+S", key("S", { ctrl: true, shift: true }), "toggleSidebar"],
  ["Ctrl+Shift+M", key("M", { ctrl: true, shift: true }), "toggleLayout"],
  ["Ctrl+Shift+PageDown", key("PageDown", { ctrl: true, shift: true }), "nextPane"],
  ["Ctrl+Shift+PageUp", key("PageUp", { ctrl: true, shift: true }), "prevPane"],
])("%s is %s by default", (_name, event, action) => {
  expect(matchShortcut(event, "terminal", B)).toBe(action);
});
