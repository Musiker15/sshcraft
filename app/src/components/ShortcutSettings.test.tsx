import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { SettingsProvider } from "../settings/SettingsProvider";
import { ShortcutSettings } from "./ShortcutSettings";

const calls = vi.hoisted(() => ({ saved: [] as Array<Record<string, unknown>> }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { settings: Record<string, unknown> }) => {
    switch (command) {
      case "get_settings":
        return {
          language: "en",
          app_theme: "dark",
          terminal_theme: "sshcraft-dark",
          font_family: "Consolas",
          font_size: 14,
          host_view: "grid",
          terminal_type: "xterm-256color",
          clipboard_osc52: "write",
          shortcuts: {},
        };
      case "save_settings":
        calls.saved.push(args!.settings);
        return args!.settings;
      default:
        return null;
    }
  }),
}));

beforeEach(() => {
  calls.saved = [];
});
afterEach(cleanup);

async function renderSettings() {
  render(
    <SettingsProvider>
      <ShortcutSettings />
    </SettingsProvider>,
  );
  return screen.findByText(en["shortcuts.action.newTab"]);
}

async function record(action: string, key: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean }) {
  const label = await renderSettings();
  const row = label.closest("li") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: en["shortcuts.change"] }));
  await act(async () => {
    fireEvent.keyDown(window, { key, ctrlKey: mods.ctrl ?? false, shiftKey: mods.shift ?? false, altKey: mods.alt ?? false });
  });
  void action;
  return row;
}

test("recording a combination saves it for the action", async () => {
  await record("newTab", "N", { ctrl: true, shift: true });
  expect(calls.saved[calls.saved.length - 1]?.shortcuts).toEqual({ newTab: ["Ctrl+Shift+N"] });
});

test("a combination another action has is refused with its name", async () => {
  const row = await record("newTab", "P", { ctrl: true, shift: true });
  expect(calls.saved).toHaveLength(0);
  expect(within(row).getByText(/Command palette/)).toBeTruthy();
});

test("a combination that is not terminal-safe is marked as outside the terminal", async () => {
  const row = await record("newTab", "w", { ctrl: true });
  expect(calls.saved[calls.saved.length - 1]?.shortcuts).toEqual({ newTab: ["Ctrl+W"] });
  expect(within(row).getByText(en["shortcuts.appOnly"])).toBeTruthy();
});

test("a key without Ctrl, Alt or Meta is refused", async () => {
  const row = await record("newTab", "q", {});
  expect(calls.saved).toHaveLength(0);
  expect(within(row).getByText(en["shortcuts.invalid"])).toBeTruthy();
});
