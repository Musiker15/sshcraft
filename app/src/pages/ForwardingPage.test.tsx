import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ForwardStatus, Host } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { ForwardingPage } from "./ForwardingPage";

const state = vi.hoisted(() => ({
  statuses: [] as unknown[],
  calls: [] as Array<{ command: string; args: unknown }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: unknown) => {
    state.calls.push({ command, args });
    if (command === "forward_statuses") return state.statuses;
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

beforeEach(() => {
  state.statuses = [];
  state.calls = [];
});
afterEach(cleanup);

const hosts: Host[] = [
  {
    id: "h1",
    name: "db-server",
    group_id: null,
    settings: { address: "10.0.0.5", port: null, username: "root", key: null },
    tags: [],
    os: null,
    forwards: [
      {
        id: "r1",
        name: "",
        kind: "local",
        bind_address: "127.0.0.1",
        bind_port: 15432,
        target_host: "db",
        target_port: 5432,
      },
    ],
  },
];

async function show(statuses: ForwardStatus[] = []) {
  state.statuses = statuses;
  render(
    <I18nProvider language="en">
      <ForwardingPage hosts={hosts} onChanged={() => {}} />
    </I18nProvider>,
  );
  return screen.findByText("-L 127.0.0.1:15432:db:5432");
}

const calledWith = (command: string) => state.calls.filter((c) => c.command === command).map((c) => c.args);

test("a double click starts a stopped rule", async () => {
  const row = await show();
  fireEvent.doubleClick(row);
  await vi.waitFor(() => expect(calledWith("forward_start")).toEqual([{ hostId: "h1", ruleId: "r1" }]));
});

test("a double click stops a running rule, which offers no editing", async () => {
  const row = await show([
    { rule_id: "r1", host_id: "h1", state: "running", bound_port: 15432, connections: 2 },
  ]);
  await screen.findByText((text) => text.startsWith(en["forwarding.state.running"]));
  fireEvent.doubleClick(row);
  await vi.waitFor(() => expect(calledWith("forward_stop")).toEqual([{ ruleId: "r1" }]));
  fireEvent.contextMenu(row);
  expect(screen.queryByText(en["forwarding.edit"])).toBeNull();
  expect(screen.getByText(en["forwarding.stop"])).toBeTruthy();
});

test("a failed rule shows why", async () => {
  await show([
    { rule_id: "r1", host_id: "h1", state: "failed", code: "port_in_use", detail: null },
  ]);
  expect(await screen.findByText(en["errors.port_in_use"])).toBeTruthy();
});
