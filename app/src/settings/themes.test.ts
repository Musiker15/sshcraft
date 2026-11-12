import { expect, test } from "vitest";
import { resolveAppTheme, TERMINAL_THEMES, terminalTheme } from "./themes";

const FIELDS = [
  "background", "foreground", "cursor", "selectionBackground",
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

test("every terminal theme has all 20 colours as #rrggbb", () => {
  for (const theme of TERMINAL_THEMES) {
    for (const field of FIELDS) expect([theme.id, field, theme.colors[field]]).toEqual([theme.id, field, expect.stringMatching(/^#[0-9a-f]{6}$/)]);
  }
});

test("theme ids are unique and include the default", () => {
  const ids = TERMINAL_THEMES.map((t) => t.id);
  expect(new Set(ids).size).toBe(ids.length);
  expect(ids).toEqual(["sshcraft-dark", "sshcraft-light", "solarized-dark", "solarized-light", "nord", "dracula"]);
});

test("unknown theme falls back to the default", () => {
  expect(terminalTheme("missing").id).toBe("sshcraft-dark");
  expect(terminalTheme("nord").id).toBe("nord");
});

test("app theme follows the system only when set to system", () => {
  expect(resolveAppTheme("system", true)).toBe("dark");
  expect(resolveAppTheme("system", false)).toBe("light");
  expect(resolveAppTheme("light", true)).toBe("light");
  expect(resolveAppTheme("dark", false)).toBe("dark");
});
