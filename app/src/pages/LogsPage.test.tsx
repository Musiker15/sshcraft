import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Host, LogSummary } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { LogsPage, formatSize } from "./LogsPage";

const state = vi.hoisted(() => ({
  logs: [] as unknown[],
  calls: [] as { command: string; args: unknown }[],
  hits: [] as unknown[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: unknown) => {
    state.calls.push({ command, args });
    if (command === "logs_list") return state.logs;
    if (command === "list_servers") return [{ id: "srv", name: "home" }];
    if (command === "logs_search") return state.hits;
    if (command === "log_read") return new ArrayBuffer(0);
    return null;
  }),
}));
vi.mock("../components/LogViewer", () => ({
  LogViewer: ({ log, query }: { log: LogSummary; query?: string }) => (
    <div data-testid="viewer">
      {log.title}|{query}
    </div>
  ),
}));

const log = (id: string, title: string, extra: Partial<LogSummary> = {}): LogSummary => ({
  id,
  title,
  kind: "ssh",
  host_id: "h1",
  started_at: Date.UTC(2026, 10, 13, 8, 0),
  ended_at: Date.UTC(2026, 10, 13, 8, 30),
  size: 2048,
  truncated: false,
  marks: [],
  location: null,
  ...extra,
});

const hosts: Host[] = [
  { id: "h1", name: "web", group_id: null, settings: { address: null, port: null, username: null, key: null } },
  { id: "h2", name: "db", group_id: null, settings: { address: null, port: null, username: null, key: null } },
];

beforeEach(() => {
  state.logs = [
    log("1", "web"),
    log("2", "db", { host_id: "h2", location: "srv" }),
    log("3", "PowerShell", { kind: "local", host_id: null, ended_at: null }),
  ];
  state.calls = [];
  state.hits = [];
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function show() {
  render(
    <I18nProvider language="en">
      <LogsPage hosts={hosts} />
    </I18nProvider>,
  );
  await screen.findByText("PowerShell");
}

test("rows show host, duration, size and place", async () => {
  await show();
  const web = screen.getByRole("button", { name: "web" }).closest(".log-row") as HTMLElement;
  expect(within(web).getByText("30 minutes")).toBeTruthy();
  expect(within(web).getByText("2 KB")).toBeTruthy();
  expect(within(web).getByText(en["logs.thisDevice"])).toBeTruthy();
  const db = screen.getByRole("button", { name: "db" }).closest(".log-row") as HTMLElement;
  expect(await within(db).findByText("home")).toBeTruthy();
  const local = screen.getByRole("button", { name: "PowerShell" }).closest(".log-row") as HTMLElement;
  expect(within(local).getByText(en["logs.running"])).toBeTruthy();
  expect(within(local).getByText(en["logs.kindLocal"])).toBeTruthy();
});

test("the host filter narrows the list", async () => {
  await show();
  fireEvent.change(screen.getByLabelText(en["logs.filterHost"]), { target: { value: "h2" } });
  expect(screen.queryByRole("button", { name: "web" })).toBeNull();
  expect(screen.getByRole("button", { name: "db" })).toBeTruthy();
  fireEvent.change(screen.getByLabelText(en["logs.filterHost"]), { target: { value: "" } });
  expect(screen.getByRole("button", { name: "PowerShell" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "db" })).toBeNull();
});

test("searching waits for two characters and opens a hit with the query", async () => {
  await show();
  vi.useFakeTimers();
  state.hits = [{ log_id: "2", line: 4, snippet: "Active: running" }];
  fireEvent.change(screen.getByLabelText(en["logs.search"]), { target: { value: "a" } });
  await act(async () => {
    vi.advanceTimersByTime(400);
  });
  expect(state.calls.some((c) => c.command === "logs_search")).toBe(false);
  fireEvent.change(screen.getByLabelText(en["logs.search"]), { target: { value: "active" } });
  await act(async () => {
    vi.advanceTimersByTime(400);
  });
  expect(state.calls.find((c) => c.command === "logs_search")?.args).toEqual({ query: "active" });
  vi.useRealTimers();
  fireEvent.click(await screen.findByText("Active: running"));
  expect(screen.getByTestId("viewer").textContent).toBe("db|active");
});

test("deleting selected logs asks first", async () => {
  await show();
  fireEvent.click(screen.getByLabelText("Select web"));
  fireEvent.click(screen.getByLabelText("Select db"));
  fireEvent.click(screen.getByRole("button", { name: "Delete 2" }));
  expect(state.calls.some((c) => c.command === "logs_delete")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: en["common.delete"] }));
  await vi.waitFor(() => {
    expect(state.calls.find((c) => c.command === "logs_delete")?.args).toEqual({ ids: ["1", "2"] });
  });
});

test("deleting all shown takes only the filtered logs", async () => {
  await show();
  fireEvent.change(screen.getByLabelText(en["logs.filterHost"]), { target: { value: "h1" } });
  fireEvent.click(screen.getByRole("button", { name: en["logs.deleteShown"] }));
  fireEvent.click(screen.getByRole("button", { name: en["common.delete"] }));
  await vi.waitFor(() => {
    expect(state.calls.find((c) => c.command === "logs_delete")?.args).toEqual({ ids: ["1"] });
  });
});

test("opening a log shows the viewer", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: "web" }));
  expect(screen.getByTestId("viewer").textContent).toBe("web|");
});

test("sizes read naturally", () => {
  expect(formatSize(500, "en")).toBe("500 B");
  expect(formatSize(1536, "en")).toBe("1.5 KB");
  expect(formatSize(64 * 1024 * 1024, "en")).toBe("64 MB");
});

test("while searching, deleting all shown takes only the logs of the hits", async () => {
  await show();
  vi.useFakeTimers();
  state.hits = [
    { log_id: "2", line: 1, snippet: "error a" },
    { log_id: "2", line: 5, snippet: "error b" },
  ];
  fireEvent.change(screen.getByLabelText(en["logs.search"]), { target: { value: "error" } });
  await act(async () => {
    vi.advanceTimersByTime(400);
  });
  vi.useRealTimers();
  await screen.findByText("error a");
  fireEvent.click(screen.getByRole("button", { name: en["logs.deleteShown"] }));
  fireEvent.click(screen.getByRole("button", { name: en["common.delete"] }));
  await vi.waitFor(() => {
    expect(state.calls.find((c) => c.command === "logs_delete")?.args).toEqual({ ids: ["2"] });
  });
});

test("hits of an earlier query are not shown for a new one", async () => {
  await show();
  vi.useFakeTimers();
  state.hits = [{ log_id: "1", line: 0, snippet: "old match" }];
  fireEvent.change(screen.getByLabelText(en["logs.search"]), { target: { value: "old" } });
  await act(async () => {
    vi.advanceTimersByTime(400);
  });
  expect(screen.getByText("old match")).toBeTruthy();
  fireEvent.change(screen.getByLabelText(en["logs.search"]), { target: { value: "new" } });
  expect(screen.queryByText("old match")).toBeNull();
  vi.useRealTimers();
});

test("a log being recorded cannot be selected or deleted", async () => {
  await show();
  expect((screen.getByLabelText("Select PowerShell") as HTMLInputElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText(en["logs.filterHost"]), { target: { value: "" } });
  expect(screen.getByRole("button", { name: en["logs.deleteShown"] }).hasAttribute("disabled")).toBe(true);
});
