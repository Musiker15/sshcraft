import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../../i18n/en.json";
import { I18nProvider } from "../../i18n/i18n";
import { AddServerForm } from "./AddServerForm";

const calls = vi.hoisted(() => ({ commands: [] as string[] }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    calls.commands.push(command);
    if (command === "create_sync_account") throw { code: "sync_email_taken", detail: "email_taken" };
    return null;
  }),
}));

afterEach(() => {
  cleanup();
  calls.commands = [];
});

function renderForm() {
  const onCreated = vi.fn();
  const onJoined = vi.fn();
  render(
    <I18nProvider language="en">
      <AddServerForm onCreated={onCreated} onJoined={onJoined} onCancel={() => {}} />
    </I18nProvider>,
  );
  return { onCreated, onJoined };
}

const fill = (label: string, value: string) =>
  fireEvent.change(screen.getByLabelText(label), { target: { value } });

test("required fields are checked before anything is sent", async () => {
  renderForm();
  fireEvent.click(screen.getByRole("button", { name: en["sync.create"] }));
  expect(await screen.findByText(en["sync.required"])).toBeTruthy();
  expect(calls.commands).toEqual([]);
});

test("joining asks for the account key instead of name and email", () => {
  renderForm();
  expect(screen.queryByLabelText(en["sync.accountKey"])).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: en["sync.modeJoin"] }));
  expect(screen.getByLabelText(en["sync.accountKey"])).toBeTruthy();
  expect(screen.queryByLabelText(en["sync.email"])).toBeNull();
});

test("a server error shows translated", async () => {
  renderForm();
  fill(en["sync.serverName"], "home");
  fill(en["sync.origin"], "https://sync.example.org");
  fill(en["sync.apiKey"], "key");
  fill(en["sync.name"], "Alice");
  fill(en["sync.email"], "a@example.org");
  fill(en["sync.deviceName"], "laptop");
  fireEvent.click(screen.getByRole("button", { name: en["sync.create"] }));
  expect(await screen.findByText(en["errors.sync_email_taken"])).toBeTruthy();
  expect(calls.commands).toEqual(["create_sync_account"]);
});
