import { expect, test } from "vitest";
import { classifyPrompt, emptyLine, shouldRecord, track, type Line } from "./lineTracker";

const SHELL = "me@web:~$ ";

function type(prompt: string, ...chunks: string[]): { line: Line; committed: (string | null)[]; kinds: (string | null)[] } {
  let line = emptyLine;
  const committed: (string | null)[] = [];
  const kinds: (string | null)[] = [];
  for (const chunk of chunks) {
    const next = track(line, chunk, prompt);
    if (chunk.includes("\r")) {
      committed.push(next.committed);
      kinds.push(line.kind);
    }
    line = next.line;
  }
  return { line, committed, kinds };
}

test("typing, backspace and enter give the command", () => {
  const { line, committed, kinds } = type(SHELL, "l", "s", "x", "\x7f", " ", "-", "l", "a", "\r");
  expect(committed).toEqual(["ls -la"]);
  expect(kinds).toEqual(["shell"]);
  expect(line).toEqual(emptyLine);
});

test("pasted text without a line break joins the line", () => {
  expect(type(SHELL, "git ", "status --short").line).toMatchObject({ text: "git status --short", certain: true });
});

test("Ctrl+U and Ctrl+C start the line over", () => {
  expect(type(SHELL, "rm -rf /", "\x15", "ls", "\r").committed).toEqual(["ls"]);
  expect(type(SHELL, "\t", "\x03", "pwd", "\r").committed).toEqual(["pwd"]);
});

test.each([
  ["Tab completion", ["cd /et", "\t", "\r"]],
  ["arrow up for shell history", ["\x1b[A", "\r"]],
  ["Ctrl+R search", ["\x12", "ssh", "\r"]],
  ["cursor left inside the line", ["git sttus", "\x1b[D", "\r"]],
])("%s makes the line unknown, nothing is committed", (_name, chunks) => {
  expect(type(SHELL, ...chunks).committed).toEqual([null]);
});

test("after an unknown line the next one is known again", () => {
  expect(type(SHELL, "\x1b[A", "\r", "uptime", "\r").committed).toEqual([null, "uptime"]);
});

test("pasting several lines records none of them", () => {
  const pasted = type(SHELL, "echo a\recho b\r");
  expect(pasted.committed).toEqual([null]);
  expect(pasted.line).toEqual(emptyLine);
  expect(type(SHELL, "echo a\recho b").line.certain).toBe(false);
});

test("a bracketed paste is one known insertion", () => {
  expect(type(SHELL, "\x1b[200~git log\x1b[201~", "\r").committed).toEqual(["git log"]);
  expect(type(SHELL, "\x1b[200~a\rb\x1b[201~").line.certain).toBe(false);
});

test.each([
  ["[sudo] password for me: ", "secret"],
  ["Password:", "secret"],
  ["[sudo] Passwort für moritz: ", "secret"],
  ["Enter pass phrase for key.pem:", "secret"],
  ["Enter passphrase for key '/home/me/.ssh/id_ed25519': ", "secret"],
  ["Mot de passe : ", "secret"],
  ["Contraseña: ", "secret"],
  ["Token (will be hidden): ", "secret"],
  ["Verification code: ", "secret"],
  [SHELL, "shell"],
  ["root@web:/srv# ", "shell"],
  ["PS C:\\Users\\me> ", "shell"],
  ["C:\\Users\\me>", "shell"],
  ["❯ ", "shell"],
  ["mysql> ", "other"],
  ["MariaDB [(none)]> ", "other"],
  ["postgres=# ", "other"],
  ["app=> ", "other"],
  [">>> ", "other"],
  ["... ", "other"],
  ["irb(main):001:0> ", "other"],
  ["sqlite> ", "other"],
  ["> ", "other"],
  ["some output without prompt", "other"],
])("prompt %j is %s", (prompt, kind) => {
  expect(classifyPrompt(`earlier output\r\n${prompt}`)).toBe(kind);
});

test("the kind is decided when the line starts and kept until Enter", () => {
  // With pwfeedback the asterisks come after the prompt: still a secret line.
  let line = emptyLine;
  line = track(line, "h", "[sudo] password for me: ").line;
  expect(line.kind).toBe("secret");
  // Whatever the output shows meanwhile (asterisks, a program's echo), the line stays secret.
  line = track(line, "unter2", "h").line;
  expect(line.kind).toBe("secret");
  expect(track(line, "\r", "h").committed).toBe("hunter2");
});

test.each([
  [" ls", "shell", false],
  ["hunter2", "secret", false],
  ["SELECT 1;", "other", false],
  ["uptime", "shell", true],
  ["   ", "shell", false],
] as const)("%j on a %s line recorded: %s", (command, kind, expected) => {
  expect(shouldRecord(command, { kind, alternate: false, broadcast: false })).toBe(expected);
});

test("full-screen programs and broadcast are never recorded", () => {
  expect(shouldRecord("ls", { kind: "shell", alternate: true, broadcast: false })).toBe(false);
  expect(shouldRecord("ls", { kind: "shell", alternate: false, broadcast: true })).toBe(false);
});
