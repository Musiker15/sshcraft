import { expect, test } from "vitest";
import { emptyLine, shouldRecord, track, type Line } from "./lineTracker";

function type(...chunks: string[]): { line: Line; committed: (string | null)[] } {
  let line = emptyLine;
  const committed: (string | null)[] = [];
  for (const chunk of chunks) {
    const next = track(line, chunk);
    line = next.line;
    if (chunk.includes("\r")) committed.push(next.committed);
  }
  return { line, committed };
}

test("typing, backspace and enter give the command", () => {
  const { line, committed } = type("l", "s", "x", "\x7f", " ", "-", "l", "a", "\r");
  expect(committed).toEqual(["ls -la"]);
  expect(line).toEqual(emptyLine);
});

test("pasted text without a line break joins the line", () => {
  expect(type("git ", "status --short").line).toEqual({ text: "git status --short", certain: true });
});

test("Ctrl+U and Ctrl+C start the line over", () => {
  expect(type("rm -rf /", "\x15", "ls", "\r").committed).toEqual(["ls"]);
  expect(type("\t", "\x03", "pwd", "\r").committed).toEqual(["pwd"]);
});

test.each([
  ["Tab completion", ["cd /et", "\t", "\r"]],
  ["arrow up for shell history", ["\x1b[A", "\r"]],
  ["Ctrl+R search", ["\x12", "ssh", "\r"]],
  ["cursor left inside the line", ["git sttus", "\x1b[D", "\r"]],
])("%s makes the line unknown, nothing is committed", (_name, chunks) => {
  expect(type(...chunks).committed).toEqual([null]);
});

test("after an unknown line the next one is known again", () => {
  expect(type("\x1b[A", "\r", "uptime", "\r").committed).toEqual([null, "uptime"]);
});

test("pasting several lines records none of them", () => {
  const pasted = type("echo a\recho b\r");
  expect(pasted.committed).toEqual([null]);
  expect(pasted.line).toEqual(emptyLine);
  // A paste that ends inside a line leaves that line unknown.
  expect(type("echo a\recho b").line.certain).toBe(false);
});

test.each([
  [" ls", { secretPrompt: false, alternate: false, broadcast: false }],
  ["hunter2", { secretPrompt: true, alternate: false, broadcast: false }],
  [":wq", { secretPrompt: false, alternate: true, broadcast: false }],
  ["uptime", { secretPrompt: false, alternate: false, broadcast: true }],
  ["   ", { secretPrompt: false, alternate: false, broadcast: false }],
])("%j is not recorded in %j", (command, context) => {
  expect(shouldRecord(command, context)).toBe(false);
});

test("an ordinary command is recorded", () => {
  expect(shouldRecord("ls -la", { secretPrompt: false, alternate: false, broadcast: false })).toBe(true);
});
