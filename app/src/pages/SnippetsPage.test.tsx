import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Host, Snippet } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { SnippetsPage } from "./SnippetsPage";

const state = vi.hoisted(() => ({ snippets: [] as unknown[], saved: [] as unknown[] }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { snippet: unknown }) => {
    if (command === "snippets_list") return state.snippets;
    if (command === "snippet_save") {
      state.saved.push(args!.snippet);
      return args!.snippet;
    }
    return null;
  }),
}));

const snippet = (id: string, name: string, pkg: string): Snippet => ({
  id,
  name,
  package: pkg,
  content: `echo ${name}`,
  default_targets: [],
});

beforeEach(() => {
  state.snippets = [snippet("1", "Updates", "Wartung"), snippet("2", "Logs", "Docker"), snippet("3", "Uptime", "")];
  state.saved = [];
});
afterEach(cleanup);

const hosts: Host[] = [
  { id: "h1", name: "web", group_id: null, settings: { address: null, port: null, username: null, key: null } },
];

async function show(onRun = vi.fn()) {
  render(
    <I18nProvider language="en">
      <SnippetsPage hosts={hosts} onRun={onRun} />
    </I18nProvider>,
  );
  await screen.findByText("Updates");
  return onRun;
}

test("packages filter the list", async () => {
  await show();
  const packages = screen.getByRole("list", { name: en["snippets.packages"] });
  fireEvent.click(within(packages).getByRole("button", { name: "Docker" }));
  expect(screen.queryByText("Updates")).toBeNull();
  expect(screen.getByText("Logs")).toBeTruthy();
  fireEvent.click(within(packages).getByRole("button", { name: en["snippets.noPackage"] }));
  expect(screen.getByText("Uptime")).toBeTruthy();
  expect(screen.queryByText("Logs")).toBeNull();
});

test("a new snippet is saved with its package and default hosts", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: en["snippets.new"] }));
  fireEvent.change(screen.getByLabelText(en["snippets.name"]), { target: { value: "Disk" } });
  fireEvent.change(screen.getByLabelText(en["snippets.package"]), { target: { value: "Wartung" } });
  fireEvent.change(screen.getByLabelText(en["snippets.content"]), { target: { value: "df -h\ndu -sh /var" } });
  fireEvent.click(screen.getByRole("checkbox", { name: "web" }));
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(state.saved).toHaveLength(1));
  expect(state.saved[0]).toMatchObject({
    name: "Disk",
    package: "Wartung",
    content: "df -h\ndu -sh /var",
    default_targets: ["h1"],
  });
});

test("run hands the snippet over", async () => {
  const onRun = await show();
  const row = screen.getByText("Logs").closest(".snippet-row") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: en["snippets.run"] }));
  expect(onRun).toHaveBeenCalledWith(expect.objectContaining({ id: "2" }));
});
