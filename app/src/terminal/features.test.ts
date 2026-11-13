import { expect, test } from "vitest";
import { expectsInput, isOpenableLink, isTerminalReport, osc52Text, osc7Path, startupInput } from "./features";

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

test("OSC 52 writes decode as UTF-8", () => {
  expect(osc52Text(`c;${b64("hello")}`)).toBe("hello");
  expect(osc52Text(`;${b64("hä ß")}`)).toBe("hä ß");
  expect(osc52Text(`p;${b64("x")}`)).toBe("x");
});

test("OSC 52 queries, empty, broken and oversized payloads are ignored", () => {
  expect(osc52Text("c;?")).toBeNull();
  expect(osc52Text("c;")).toBeNull();
  expect(osc52Text("c;@@not base64@@")).toBeNull();
  expect(osc52Text("nothing")).toBeNull();
  expect(osc52Text(`c;${b64("x".repeat(100))}`, 50)).toBeNull();
  expect(osc52Text(`c;${b64("x".repeat(50))}`, 50)).toBe("x".repeat(50));
});

test("OSC 7 gives the reported directory", () => {
  expect(osc7Path("file://myhost/srv/my%20app")).toBe("/srv/my app");
  expect(osc7Path("file:///C:/Users/me")).toBe("C:/Users/me");
  expect(osc7Path("http://x/y")).toBeNull();
  expect(osc7Path("garbage")).toBeNull();
});

test("an OSC 7 path with a lone percent sign gives nothing instead of throwing", () => {
  expect(() => osc7Path("file://h/tmp/50% off")).not.toThrow();
  expect(osc7Path("file://h/tmp/50% off")).toBeNull();
  expect(osc7Path("file://h/50%")).toBeNull();
});

test("a home-relative directory keeps its tilde unquoted", () => {
  expect(startupInput(host, { cwd: "~/my app" })).toBe("cd -- ~/'my app'\r");
  expect(startupInput(host, { cwd: "~" })).toBe("cd -- ~\r");
  expect(startupInput(host, { cwd: "~other/x" })).toBe("cd -- '~other/x'\r");
});

test("only web links are opened", () => {
  expect(isOpenableLink("https://example.com/a?b=c")).toBe(true);
  expect(isOpenableLink("http://10.0.0.1:8080/")).toBe(true);
  for (const bad of [
    "javascript:alert(1)",
    "file:///C:/Windows/system32/calc.exe",
    "ftp://example.com",
    "mailto:a@example.com",
    "https://",
    `https://example.com/${"a".repeat(2048)}`,
  ]) {
    expect(isOpenableLink(bad)).toBe(false);
  }
});

test.each([
  ["root@web's password: "],
  ["Password:"],
  ["\x1b[1mroot@x's password: \x1b[0m"],
  ["Enter passphrase for key '/home/me/.ssh/id_ed25519': "],
  ["[sudo] Passwort für moritz: "],
  ["Kennwort:"],
  ["Do you want to continue? [Y/n] "],
  ["Proceed [y/N]?"],
  ["Are you sure you want to continue connecting (yes/no/[fingerprint])? "],
  ["Overwrite (yes/no)?"],
  ["Continue?"],
])("%j waits for input", (tail) => {
  expect(expectsInput(`some output\r\n${tail}`)).toBe(true);
});

test.each([["ls -la\r\n"], ["building...\r\n"], ["me@host:~$ "], ["password changed\r\n"], [""]])(
  "%j does not wait for input",
  (tail) => {
    expect(expectsInput(tail)).toBe(false);
  },
);

const host = { kind: "host" as const, host_id: "h" };
const local = { kind: "local" as const, shell: { program: "cmd.exe", args: [], label: "cmd" } };
const serial = {
  kind: "serial" as const,
  settings: { port: "COM3", baud: 9600, data_bits: 8, stop_bits: 1, parity: "none" as const, flow: "none" as const },
};

test("an SSH pane changes directory with POSIX quoting, then runs the command", () => {
  expect(startupInput(host, { cwd: "/srv/it's", command: "htop" })).toBe("cd -- '/srv/it'\\''s' && htop\r");
  expect(startupInput(host, { cwd: "/srv" })).toBe("cd -- '/srv'\r");
  expect(startupInput(host, { command: "tail -f log" })).toBe("tail -f log\r");
  expect(startupInput(host, { cwd: "  ", command: "" })).toBeNull();
  expect(startupInput(host, undefined)).toBeNull();
});

test("local and serial panes only get the command typed", () => {
  expect(startupInput(local, { cwd: "C:\\work" })).toBeNull();
  expect(startupInput(local, { cwd: "C:\\work", command: "dir" })).toBe("dir\r");
  expect(startupInput(serial, { cwd: "/x" })).toBeNull();
  expect(startupInput(serial, { command: "AT" })).toBe("AT\r");
});

test.each([
  ["\x1b[12;40R"],
  ["\x1b[?1;2c"],
  ["\x1b[>0;276;0c"],
  ["\x1b[I"],
  ["\x1b[O"],
  ["\x1b]11;rgb:0000/0000/0000\x1b\\"],
  ["\x1b[?2004;1$y"],
  // DECRQSS answer, DECXCPR, DSR status, window size report (all sent by xterm 6).
  ["\x1bP1$r0;1m\x1b\\"],
  ["\x1b[?12;40R"],
  ["\x1b[0n"],
  ["\x1b[8;24;80t"],
  // Mouse reports (SGR and X10): a click in one pane is not replayed in the others.
  ["\x1b[<0;12;5M"],
  ["\x1b[<0;12;5m"],
  ["\x1b[M !!"],
])("%j is the terminal answering, not typing", (data) => {
  expect(isTerminalReport(data)).toBe(true);
});

test.each([["ls\r"], ["a"], ["\x1b[A"], ["\x1b[1;5C"], ["\x1b"], ["\x1bOP"], ["\x1b[200~pasted\x1b[201~"]])(
  "%j is typed by the user",
  (data) => {
    expect(isTerminalReport(data)).toBe(false);
  },
);

