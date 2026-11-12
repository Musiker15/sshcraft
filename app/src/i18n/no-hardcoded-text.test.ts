// Every user-facing text must come from the translation files, never from the component code.
import { expect, test } from "vitest";

const sources = import.meta.glob(["../**/*.tsx", "!../**/*.test.tsx"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Files that still carry fixed texts; each UI task removes the files it migrates. */
const PENDING = new Set<string>([
  "../App.tsx",
  "../components/GroupForm.tsx",
  "../components/HostForm.tsx",
  "../components/HostKeyDialog.tsx",
  "../components/KeyManager.tsx",
  "../components/SettingsFields.tsx",
  "../components/Sidebar.tsx",
  "../components/TerminalTabs.tsx",
]);

const ALLOWED = new Set(["SSHCraft"]);
const LETTERS = /[A-Za-zÄÖÜäöüß]{2,}/;

export function findHardcodedText(source: string): string[] {
  const findings: string[] = [];
  // JSX text between a closing ">" and the next "<", skipping code fragments.
  for (const match of source.matchAll(/>([^<>{}]*)</g)) {
    const text = match[1].trim();
    if (!text || /[=;]|&&|\|\|/.test(text)) continue;
    if (LETTERS.test(text) && !ALLOWED.has(text)) findings.push(text);
  }
  for (const match of source.matchAll(/\b(title|placeholder|aria-label|alt|label)="([^"]*)"/g)) {
    if (LETTERS.test(match[2]) && !ALLOWED.has(match[2])) findings.push(`${match[1]}="${match[2]}"`);
  }
  for (const match of source.matchAll(/\b(alert|confirm)\(/g)) findings.push(`${match[1]}(`);
  return findings;
}

test("no fixed user-facing text outside the translation files", () => {
  const offenders = Object.entries(sources)
    .filter(([path]) => !PENDING.has(path))
    .map(([path, source]) => [path, findHardcodedText(source)] as const)
    .filter(([, found]) => found.length > 0);
  expect(offenders).toEqual([]);
});

test("every pending file still exists", () => {
  for (const path of PENDING) expect(Object.keys(sources)).toContain(path);
});

test("the scanner finds JSX text, attributes and dialogs", () => {
  expect(findHardcodedText(`<button title="Löschen">Neuer Host</button>`)).toEqual(["Neuer Host", 'title="Löschen"']);
  expect(findHardcodedText(`if (!confirm(message)) return;`)).toEqual(["confirm("]);
  expect(findHardcodedText(`<span>{t("a")}</span><b>SSHCraft</b>`)).toEqual([]);
});
