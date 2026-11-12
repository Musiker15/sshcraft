import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Group, Host } from "../api";
import en from "../i18n/en.json";
import { SettingsProvider } from "../settings/SettingsProvider";
import { HostsPage } from "./HostsPage";
import { NewTabPage } from "./NewTabPage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) =>
    command === "get_settings"
      ? {
          language: "en",
          app_theme: "dark",
          terminal_theme: "sshcraft-dark",
          font_family: "Consolas",
          font_size: 14,
          host_view: "grid",
        }
      : command === "system_locale"
        ? "en-US"
        : null,
  ),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const empty = { address: null, port: null, username: null, key: null };
// Two groups share a name, a third is named like the section for hosts without a group.
const groups: Group[] = [
  { id: "g1", name: "Prod", defaults: empty },
  { id: "g2", name: "Prod", defaults: empty },
  { id: "g3", name: en["hosts.ungrouped"], defaults: empty },
];
const hosts: Host[] = [
  { id: "h1", name: "a", group_id: "g1", settings: empty },
  { id: "h2", name: "b", group_id: "g2", settings: empty },
  { id: "h3", name: "c", group_id: "g3", settings: empty },
  { id: "h4", name: "d", group_id: null, settings: empty },
];

function duplicateKeyWarnings(spy: { mock: { calls: unknown[][] } }) {
  return spy.mock.calls.filter((call) => String(call[0]).includes("same key"));
}

test("groups with the same name do not share React keys on the host page", async () => {
  const spy = vi.spyOn(console, "error").mockImplementation(() => {});
  const noop = () => {};
  render(
    <SettingsProvider>
      <HostsPage
        groups={groups}
        hosts={hosts}
        onConnect={noop}
        onEditHost={noop}
        onDeleteHost={noop}
        onEditGroup={noop}
        onDeleteGroup={noop}
        onNewHost={noop}
        onNewGroup={noop}
        onImportSshConfig={noop}
      />
    </SettingsProvider>,
  );
  await screen.findAllByText("Prod");
  expect(screen.getByText("a")).toBeTruthy();
  expect(screen.getByText("b")).toBeTruthy();
  expect(duplicateKeyWarnings(spy)).toEqual([]);
});

test("groups with the same name do not share React keys on the new-tab page", () => {
  const spy = vi.spyOn(console, "error").mockImplementation(() => {});
  render(<NewTabPage hosts={hosts} groups={groups} recent={[]} onPick={() => {}} />);
  expect(screen.getByText("a")).toBeTruthy();
  expect(duplicateKeyWarnings(spy)).toEqual([]);
});
