import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Host, Snippet } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { RunSnippetDialog } from "./RunSnippetDialog";

afterEach(cleanup);

const host = (id: string, name: string): Host => ({
  id,
  name,
  group_id: null,
  settings: { address: null, port: null, username: null, key: null },
});
const hosts = [host("h1", "web"), host("h2", "db")];
const snippet: Snippet = { id: "s", name: "Uptime", package: "", content: "uptime", default_targets: ["h2"] };
const panes = [
  { paneId: "p1", title: "web", sessionId: "s1", hostId: "h1" },
  { paneId: "p2", title: "db", sessionId: "s2", hostId: "h2" },
];

function show() {
  const onRun = vi.fn();
  render(
    <I18nProvider language="en">
      <RunSnippetDialog snippet={snippet} hosts={hosts} panes={panes} onRun={onRun} onCancel={() => {}} />
    </I18nProvider>,
  );
  return onRun;
}

test("default targets are picked, as new connection and as open pane", () => {
  show();
  const checked = (name: string) =>
    screen.getAllByRole("checkbox", { name }).map((c) => (c as HTMLInputElement).checked);
  expect(checked("db")).toEqual([true, true]);
  expect(checked("web")).toEqual([false, false]);
});

test("run passes the chosen hosts and panes", () => {
  const onRun = show();
  fireEvent.click(screen.getAllByRole("checkbox", { name: "db" })[0]);
  fireEvent.click(screen.getAllByRole("checkbox", { name: "web" })[1]);
  fireEvent.click(screen.getByRole("button", { name: en["snippets.run"] }));
  expect(onRun).toHaveBeenCalledWith({ hostIds: [], paneIds: ["p2", "p1"], run: true });
});

test("paste instead of run, and nothing chosen blocks it", () => {
  const onRun = show();
  fireEvent.click(screen.getByRole("radio", { name: en["snippets.modePaste"] }));
  fireEvent.click(screen.getByRole("button", { name: en["snippets.run"] }));
  expect(onRun).toHaveBeenCalledWith({ hostIds: ["h2"], paneIds: ["p2"], run: false });
  for (const box of screen.getAllByRole("checkbox", { name: "db" })) fireEvent.click(box);
  expect((screen.getByRole("button", { name: en["snippets.run"] }) as HTMLButtonElement).disabled).toBe(true);
});
