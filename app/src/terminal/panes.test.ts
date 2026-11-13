import { expect, test } from "vitest";
import { pasteBytes, paneInput, registerPane } from "./panes";

test("with bracketed paste the shell gets the text as one paste", () => {
  expect(pasteBytes("cd /srv\ngit pull\n", false, true)).toBe("\x1b[200~cd /srv\rgit pull\x1b[201~");
  expect(pasteBytes("uptime", true, true)).toBe("\x1b[200~uptime\x1b[201~\r");
});

test("without it, line ends become Enter and tabs do not trigger completion", () => {
  expect(pasteBytes("a\r\n\tb\n", true, false)).toBe("a\r    b\r");
  expect(pasteBytes("a\nb", false, false)).toBe("a\rb");
});

test("panes register while mounted", () => {
  const input = { track: () => {}, paste: () => {} };
  const stop = registerPane("p", input);
  expect(paneInput("p")).toBe(input);
  stop();
  expect(paneInput("p")).toBeUndefined();
});
