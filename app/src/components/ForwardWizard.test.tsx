import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Host } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { ForwardWizard } from "./ForwardWizard";

const calls = vi.hoisted(() => ({ saved: [] as Host[], stored: null as Host[] | null }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { host: Host }) => {
    if (command === "save_host") calls.saved.push(args!.host);
    if (command === "list_hosts") return calls.stored;
    return null;
  }),
}));

beforeEach(() => {
  calls.saved = [];
  calls.stored = null;
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
    forwards: [],
  },
];

function show(editing?: { hostId: string; rule: NonNullable<Host["forwards"]>[number] }) {
  const done = vi.fn();
  render(
    <I18nProvider language="en">
      <ForwardWizard hosts={hosts} editing={editing} onDone={done} onCancel={() => {}} />
    </I18nProvider>,
  );
  return done;
}

const next = () => fireEvent.click(screen.getByRole("button", { name: en["forwarding.next"] }));
const field = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });

function toDetails(kind: "local" | "remote" | "dynamic") {
  const label = en[`forwarding.kind.${kind}`];
  fireEvent.click(screen.getByRole("button", { name: (name) => name.startsWith(label) }));
  next();
  field(en["forwarding.host"], "h1");
  next();
}

test("a local rule walks through all steps and is saved on its host", async () => {
  const done = show();
  toDetails("local");
  field(en["forwarding.bindPort"], "15432");
  field(en["forwarding.targetHost"], "db");
  field(en["forwarding.targetPort"], "5432");
  next();
  expect(screen.getByText("-L 127.0.0.1:15432:db:5432")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(done).toHaveBeenCalled());
  expect(calls.saved[0].id).toBe("h1");
  expect(calls.saved[0].forwards).toMatchObject([
    { kind: "local", bind_address: "127.0.0.1", bind_port: 15432, target_host: "db", target_port: 5432 },
  ]);
});

test("listening on all addresses shows a warning", () => {
  show();
  toDetails("dynamic");
  field(en["forwarding.bindAddress"], "0.0.0.0");
  expect(screen.getByText(en["forwarding.exposedWarning"])).toBeTruthy();
});

test("an invalid port keeps the wizard on its step", () => {
  show();
  toDetails("local");
  field(en["forwarding.bindPort"], "70000");
  field(en["forwarding.targetHost"], "db");
  field(en["forwarding.targetPort"], "5432");
  expect(screen.getByText(en["forwarding.error.bindPort"])).toBeTruthy();
  expect((screen.getByRole("button", { name: en["forwarding.next"] }) as HTMLButtonElement).disabled).toBe(true);
});

test("saving starts from the host as stored now, not as shown", async () => {
  calls.stored = [{ ...hosts[0], name: "renamed elsewhere" }];
  const done = show();
  toDetails("dynamic");
  next();
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(done).toHaveBeenCalled());
  expect(calls.saved[0].name).toBe("renamed elsewhere");
});

test("editing a rule that was removed meanwhile says so instead of saving", async () => {
  calls.stored = [{ ...hosts[0], forwards: [] }];
  const rule = { id: "r9", name: "", kind: "dynamic" as const, bind_address: "127.0.0.1", bind_port: 1080, target_host: "", target_port: 0 };
  const done = show({ hostId: "h1", rule });
  next();
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  expect(await screen.findByText(en["errors.not_found"])).toBeTruthy();
  expect(done).not.toHaveBeenCalled();
  expect(calls.saved).toHaveLength(0);
});
