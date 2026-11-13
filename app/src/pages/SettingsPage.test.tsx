import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import de from "../i18n/de.json";
import en from "../i18n/en.json";
import { SettingsProvider } from "../settings/SettingsProvider";
import { SettingsPage } from "./SettingsPage";

const calls = vi.hoisted(() => ({ saved: [] as Array<Record<string, unknown>>, cleared: [] as unknown[] }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { settings: Record<string, unknown> }) => {
    switch (command) {
      case "get_settings":
        return {
          language: "de",
          app_theme: "dark",
          terminal_theme: "sshcraft-dark",
          font_family: "Consolas",
          font_size: 14,
          host_view: "grid",
          terminal_type: "xterm-256color",
          clipboard_osc52: "write",
          shortcuts: {},
          history: true,
          autocomplete: true,
          session_logs: false,
          log_retention_days: 0,
        };
      case "history_clear":
        calls.cleared.push(args);
        return null;
      case "system_locale":
        return "de-DE";
      case "save_settings": {
        const settings = args!.settings;
        calls.saved.push(settings);
        // Like Rust: keep the font size in range.
        return { ...settings, font_size: Math.min(32, Math.max(8, Number(settings.font_size))) };
      }
      default:
        return null;
    }
  }),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.1.0") }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

afterEach(cleanup);

async function renderPage() {
  render(
    <SettingsProvider>
      <SettingsPage />
    </SettingsProvider>,
  );
  return screen.findByRole("heading", { level: 1 });
}

test("switching the language saves it and translates the page at once", async () => {
  expect((await renderPage()).textContent).toBe(de["nav.settings"]);
  fireEvent.change(screen.getByLabelText(de["settings.language"]), { target: { value: "en" } });
  expect(await screen.findByRole("heading", { level: 1, name: en["nav.settings"] })).toBeTruthy();
  expect(calls.saved[calls.saved.length - 1]?.language).toBe("en");
});

test("the font size shows what Rust stored", async () => {
  await renderPage();
  const size = screen.getByLabelText(de["settings.fontSize"]) as HTMLInputElement;
  fireEvent.change(size, { target: { value: "500" } });
  fireEvent.blur(size);
  await vi.waitFor(() => expect(size.value).toBe("32"));
});

test("typing a font size saves the whole number once", async () => {
  await renderPage();
  calls.saved.length = 0;
  const size = screen.getByLabelText(de["settings.fontSize"]) as HTMLInputElement;
  fireEvent.change(size, { target: { value: "1" } });
  fireEvent.change(size, { target: { value: "16" } });
  expect(calls.saved).toHaveLength(0);
  fireEvent.keyDown(size, { key: "Enter" });
  await vi.waitFor(() => expect(calls.saved[calls.saved.length - 1]?.font_size).toBe(16));
  expect(calls.saved).toHaveLength(1);
  expect(size.value).toBe("16");
});

test("history and suggestions can be switched off, and the history cleared", async () => {
  await renderPage();
  fireEvent.click(screen.getByRole("checkbox", { name: de["settings.history"] }));
  await vi.waitFor(() => expect(calls.saved[calls.saved.length - 1]?.history).toBe(false));
  fireEvent.click(screen.getByRole("checkbox", { name: de["settings.autocomplete"] }));
  await vi.waitFor(() => expect(calls.saved[calls.saved.length - 1]?.autocomplete).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: de["settings.historyClear"] }));
  await vi.waitFor(() => expect(calls.cleared).toEqual([{ scope: null }]));
  expect(await screen.findByText(de["settings.historyCleared"])).toBeTruthy();
});

test("recording and how long logs are kept are saved", async () => {
  await renderPage();
  fireEvent.click(screen.getByRole("checkbox", { name: de["settings.sessionLogs"] }));
  await vi.waitFor(() => expect(calls.saved[calls.saved.length - 1]?.session_logs).toBe(true));
  fireEvent.change(screen.getByRole("combobox", { name: de["settings.logRetention"] }), { target: { value: "30" } });
  await vi.waitFor(() => expect(calls.saved[calls.saved.length - 1]?.log_retention_days).toBe(30));
  expect(screen.getByRole("option", { name: de["settings.logRetentionForever"] })).toBeTruthy();
  expect(screen.getByRole("option", { name: "365 Tage" })).toBeTruthy();
});
