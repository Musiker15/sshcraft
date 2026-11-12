import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Group, Host, ServerView } from "../../api";
import en from "../../i18n/en.json";
import { I18nProvider } from "../../i18n/i18n";
import { ShareDialog } from "./ShareDialog";
import { SafetyNumber } from "./SafetyNumber";

const calls = vi.hoisted(() => ({
  log: [] as { command: string; args: unknown }[],
  known: { state: "new" } as Record<string, unknown>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: unknown) => {
    calls.log.push({ command, args });
    if (command === "find_recipient")
      return {
        account: "ACCOUNT_B",
        name: "Bob",
        email: "bob@example.org",
        safety_number: "11111 22222 33333 44444 55555 66666",
        known: calls.known,
      };
    if (command === "share_items") return { area: "area-1", agent_keys_skipped: 0 };
    return null;
  }),
}));

afterEach(() => {
  cleanup();
  calls.log = [];
  calls.known = { state: "new" };
});

const server = { id: "srv", name: "home" } as ServerView;
const group: Group = { id: "g1", name: "team", defaults: {} as Group["defaults"], location: "srv" };
const host = (id: string, name: string): Host =>
  ({ id, name, group_id: "g1", settings: {} as Host["settings"], location: "srv" }) as Host;

function renderDialog() {
  const onDone = vi.fn();
  render(
    <I18nProvider language="en">
      <ShareDialog
        server={server}
        target={{ kind: "group", group }}
        hosts={[host("h1", "one"), host("h2", "two")]}
        onDone={onDone}
        onCancel={() => {}}
      />
    </I18nProvider>,
  );
  return { onDone };
}

async function lookUp() {
  fireEvent.change(screen.getByLabelText(en["share.email"]), { target: { value: "bob@example.org" } });
  fireEvent.click(screen.getByRole("button", { name: en["share.find"] }));
  await screen.findByText("Bob");
}

const shareButton = () => screen.getByRole("button", { name: en["share.share"] }) as HTMLButtonElement;

test("sharing waits until the safety number was compared", async () => {
  const { onDone } = renderDialog();
  await lookUp();
  expect(shareButton().disabled).toBe(true);
  fireEvent.click(screen.getByLabelText(en["share.compared"]));
  expect(shareButton().disabled).toBe(false);
  fireEvent.change(screen.getByLabelText(en["share.role"]), { target: { value: "write" } });
  fireEvent.click(shareButton());
  await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
  const commands = calls.log.map((c) => c.command);
  expect(commands).toEqual(["find_recipient", "confirm_recipient", "share_items"]);
  const sent = calls.log[2].args as { what: unknown; grants: unknown; withKeys: boolean };
  expect(sent.what).toEqual({ kind: "group_all", group: "g1" });
  expect(sent.grants).toEqual([{ account: "ACCOUNT_B", role: "write" }]);
  expect(sent.withKeys).toBe(false);
});

test("a changed key must be accepted on its own", async () => {
  calls.known = { state: "changed", previous_safety_number: "99999 99999 99999 99999 99999 99999" };
  renderDialog();
  await lookUp();
  expect(screen.getByText(en["share.changedWarning"].split("{name}").join("Bob"))).toBeTruthy();
  expect(screen.queryByLabelText(en["share.compared"])).toBeNull();
  expect(shareButton().disabled).toBe(true);
  fireEvent.click(screen.getByLabelText(en["share.acceptChanged"]));
  expect(shareButton().disabled).toBe(false);
});

test("a selection sends the picked hosts", async () => {
  calls.known = { state: "confirmed" };
  const { onDone } = renderDialog();
  await lookUp();
  fireEvent.click(screen.getByLabelText(en["share.selection"]));
  expect(shareButton().disabled).toBe(true);
  fireEvent.click(screen.getByLabelText("two"));
  fireEvent.click(shareButton());
  await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
  const commands = calls.log.map((c) => c.command);
  expect(commands).toEqual(["find_recipient", "share_items"]);
  expect((calls.log[1].args as { what: unknown }).what).toEqual({ kind: "selection", group: "g1", hosts: ["h2"] });
});

test("the safety number shows in six groups", () => {
  render(<SafetyNumber value="11111 22222 33333 44444 55555 66666" />);
  expect(document.querySelectorAll(".safety-group")).toHaveLength(6);
});
