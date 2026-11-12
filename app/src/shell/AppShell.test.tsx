import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { SettingsProvider } from "../settings/SettingsProvider";
import { AppShell } from "./AppShell";

const state = vi.hoisted(() => ({
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
  mounted: 0,
  unmounted: 0,
}));

vi.mock("@tauri-apps/api/core", () => {
  const responses: Record<string, unknown> = {
    list_groups: [],
    list_hosts: [
      { id: "h1", name: "web", group_id: null, settings: { address: "10.0.0.1", port: null, username: "root", key: null } },
    ],
    list_keys: [],
    list_recent: [],
    vault_status: { ok: true, code: null, detail: null },
    hello_status: { missing: false },
    system_locale: "en-US",
    get_settings: {
      language: "en",
      app_theme: "dark",
      terminal_theme: "sshcraft-dark",
      font_family: "Consolas",
      font_size: 14,
      host_view: "grid",
    },
  };
  return {
    invoke: vi.fn(async (command: string) => responses[command] ?? null),
    Channel: class {
      onmessage: unknown = null;
    },
  };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    state.handlers[name] = handler;
    return () => {};
  }),
}));

vi.mock("../components/TerminalView", async () => {
  const { useEffect } = await import("react");
  return {
    TerminalView: () => {
      useEffect(() => {
        state.mounted += 1;
        return () => {
          state.unmounted += 1;
        };
      }, []);
      return <div data-testid="terminal" />;
    },
  };
});

beforeEach(() => {
  state.mounted = 0;
  state.unmounted = 0;
  state.handlers = {};
});
afterEach(cleanup);

async function renderShell() {
  render(
    <SettingsProvider>
      <AppShell />
    </SettingsProvider>,
  );
  return screen.findByText("web");
}

test("switching to the start page and back keeps the terminal mounted", async () => {
  fireEvent.doubleClick(await renderShell());
  const sessionTab = await screen.findByRole("tab", { name: /web/ });
  expect(state.mounted).toBe(1);
  fireEvent.click(screen.getByRole("tab", { name: en["tabs.home"] }));
  expect(screen.getByRole("tab", { name: en["tabs.home"] }).getAttribute("aria-selected")).toBe("true");
  fireEvent.click(sessionTab);
  expect(sessionTab.getAttribute("aria-selected")).toBe("true");
  expect(state.mounted).toBe(1);
  expect(state.unmounted).toBe(0);
});

test("a withdrawn host key question disappears", async () => {
  await renderShell();
  act(() =>
    state.handlers["host-key-prompt"]({
      payload: { prompt_id: 7, host_name: "web", address: "10.0.0.1", port: 22, fingerprint: "SHA256:abc" },
    }),
  );
  expect(screen.getByText("SHA256:abc")).toBeTruthy();
  act(() => state.handlers["host-key-prompt-withdrawn"]({ payload: { prompt_id: 7 } }));
  expect(screen.queryByText("SHA256:abc")).toBeNull();
});
