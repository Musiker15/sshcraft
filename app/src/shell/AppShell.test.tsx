import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { SettingsProvider } from "../settings/SettingsProvider";
import { AppShell } from "./AppShell";
import { invoke } from "@tauri-apps/api/core";

type ViewProps = {
  pane: { id: string; title: string; startup?: { cwd?: string | null; command?: string | null; raw?: string } };
  onAttach: (id: string) => void;
  onInput: (data: string, report: boolean) => void;
};

const state = vi.hoisted(() => ({
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
  mounted: 0,
  unmounted: 0,
  views: {} as Record<string, ViewProps>,
}));

vi.mock("@tauri-apps/api/core", () => {
  const responses: Record<string, unknown> = {
    list_groups: [],
    list_hosts: [
      { id: "h1", name: "web", group_id: null, settings: { address: "10.0.0.1", port: null, username: "root", key: null } },
    ],
    list_keys: [],
    list_recent: [],
    local_shells: [],
    history_list: [],
    snippets_list: [{ id: "sn", name: "Uptime", package: "", content: "uptime", default_targets: ["h1"] }],
    workspace_templates: [
      {
        id: "tpl",
        name: "Ops",
        layout: "split",
        col_sizes: [],
        row_sizes: [],
        panes: [
          { target: { kind: "host", host_id: "h1" }, title: "web", cwd: "/srv", command: "htop" },
          { target: { kind: "host", host_id: "gone" }, title: "old", cwd: null, command: null },
        ],
      },
    ],
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
      terminal_type: "xterm-256color",
      clipboard_osc52: "write",
      shortcuts: {},
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
    TerminalView: (props: ViewProps) => {
      state.views[props.pane.id] = props;
      useEffect(() => {
        state.mounted += 1;
        return () => {
          state.unmounted += 1;
        };
      }, []);
      return (
        <div data-testid="terminal" className="terminal-host">
          <textarea className="xterm-helper-textarea" />
        </div>
      );
    },
  };
});

beforeEach(() => {
  state.mounted = 0;
  state.unmounted = 0;
  state.handlers = {};
  state.views = {};
  vi.mocked(invoke).mockClear();
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

const question = (id: number, fingerprint: string) => ({
  payload: { prompt_id: id, host_name: `h${id}`, address: "10.0.0.1", port: 22, fingerprint },
});

test("the next host key question starts with the focus on reject", async () => {
  await renderShell();
  act(() => {
    state.handlers["host-key-prompt"](question(7, "SHA256:seven"));
    state.handlers["host-key-prompt"](question(8, "SHA256:eight"));
  });
  const trust = screen.getByRole("button", { name: en["hostKey.trust"] });
  trust.focus();
  fireEvent.click(trust);
  expect(screen.getByText("SHA256:eight")).toBeTruthy();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: en["hostKey.reject"] }));
});

test("a question withdrawn while it is answered does not take the next one with it", async () => {
  await renderShell();
  act(() => {
    state.handlers["host-key-prompt"](question(7, "SHA256:seven"));
    state.handlers["host-key-prompt"](question(8, "SHA256:eight"));
  });
  const reject = screen.getByRole("button", { name: en["hostKey.reject"] });
  act(() => {
    state.handlers["host-key-prompt-withdrawn"]({ payload: { prompt_id: 7 } });
    fireEvent.click(reject);
  });
  expect(screen.getByText("SHA256:eight")).toBeTruthy();
});

const press = (key: string, mods: { shift?: boolean } = {}) =>
  act(() => {
    fireEvent.keyDown(window, { key, ctrlKey: true, shiftKey: mods.shift ?? true });
  });

/** Opens "web", splits and picks "web" again: one tab, two panes. */
async function twoPanes() {
  fireEvent.doubleClick(await renderShell());
  await screen.findByRole("tab", { name: /web/ });
  press("D");
  // The picker pane offers the hosts; pick "web" there.
  const picker = await screen.findByTestId("pane-picker");
  fireEvent.click(within(picker).getAllByText("web")[0]);
  expect(Object.keys(state.views)).toHaveLength(2);
  return Object.keys(state.views);
}

const writes = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "session_write")
    .map(([, args]) => (args as { id: string }).id);

test("split then detach keeps both sessions", async () => {
  await twoPanes();
  expect(state.mounted).toBe(2);
  fireEvent.click(screen.getAllByRole("button", { name: en["workspace.detach"] })[1]);
  expect(screen.getAllByRole("tab")).toHaveLength(3);
  expect(state.mounted).toBe(2);
  expect(state.unmounted).toBe(0);
});

test("broadcast writes to every pane of the tab, answers only to their own", async () => {
  const [a, b] = await twoPanes();
  act(() => {
    state.views[a].onAttach("s-a");
    state.views[b].onAttach("s-b");
  });
  act(() => state.views[a].onInput("x", false));
  expect(writes()).toEqual(["s-a"]);
  press("B");
  vi.mocked(invoke).mockClear();
  act(() => state.views[a].onInput("y", false));
  expect(writes().sort()).toEqual(["s-a", "s-b"]);
  vi.mocked(invoke).mockClear();
  act(() => state.views[b].onInput("\x1b[1;1R", true));
  expect(writes()).toEqual(["s-b"]);
});

test("dropping a tab on another merges them without reconnecting", async () => {
  fireEvent.doubleClick(await renderShell());
  fireEvent.click(screen.getByRole("tab", { name: en["tabs.home"] }));
  fireEvent.doubleClick(within(screen.getByRole("main")).getByText("web"));
  const tabs = await screen.findAllByRole("tab", { name: /web/ });
  expect(tabs).toHaveLength(2);
  const data = new Map<string, string>();
  const dataTransfer = {
    setData: (k: string, v: string) => data.set(k, v),
    getData: (k: string) => data.get(k) ?? "",
    get types() {
      return [...data.keys()];
    },
    effectAllowed: "move",
    dropEffect: "move",
  };
  fireEvent.dragStart(tabs[1], { dataTransfer });
  fireEvent.dragOver(tabs[0], { dataTransfer });
  fireEvent.drop(tabs[0], { dataTransfer });
  expect(screen.getAllByRole("tab", { name: /web/ })).toHaveLength(1);
  expect(state.mounted).toBe(2);
  expect(state.unmounted).toBe(0);
});

test("the focus layout lists every pane and shows the one picked", async () => {
  const [a, b] = await twoPanes();
  press("M");
  const list = screen.getByRole("list", { name: en["workspace.panes"] });
  const entries = within(list).getAllByRole("button");
  expect(entries).toHaveLength(2);
  fireEvent.click(entries[0]);
  expect(document.querySelector(`[data-pane="${a}"]`)?.classList.contains("offscreen")).toBe(false);
  expect(document.querySelector(`[data-pane="${b}"]`)?.classList.contains("offscreen")).toBe(true);
});

test("a template opens its panes with their start, leaving out deleted hosts", async () => {
  await renderShell();
  press("T");
  fireEvent.click(await screen.findByRole("button", { name: /^Ops/ }));
  expect(await screen.findByRole("tab", { name: /Ops/ })).toBeTruthy();
  const views = Object.values(state.views);
  expect(views).toHaveLength(1);
  expect(views[0].pane).toMatchObject({ title: "web", startup: { cwd: "/srv", command: "htop" } });
  expect(screen.getByText(en["workspaces.skipped"].replace("{count}", "1"))).toBeTruthy();
});

test("clicking a pane's title bar gives that pane's terminal the keyboard", async () => {
  const [, b] = await twoPanes();
  const pane = document.querySelector(`[data-pane="${b}"]`) as HTMLElement;
  const title = pane.querySelector(".pane-title") as HTMLElement;
  fireEvent.mouseDown(title);
  fireEvent.click(title);
  expect(document.activeElement).toBe(pane.querySelector("textarea"));
});

const written = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "session_write")
    .map(([, args]) => {
      const { id, data } = args as { id: string; data: number[] };
      return { id, text: new TextDecoder().decode(new Uint8Array(data)) };
    });

async function runDialog() {
  fireEvent.doubleClick(await renderShell());
  await screen.findByRole("tab", { name: /web/ });
  const [pane] = Object.keys(state.views);
  act(() => state.views[pane].onAttach("s-1"));
  fireEvent.click(screen.getByRole("tab", { name: en["tabs.home"] }));
  fireEvent.click(screen.getByRole("button", { name: en["nav.snippets"] }));
  const row = (await screen.findByText("Uptime")).closest(".snippet-row") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: en["snippets.run"] }));
  return within(screen.getByText(en["snippets.runTitle"].replace("{name}", "Uptime")).closest(".dialog") as HTMLElement);
}

test("a snippet runs in the open pane of its default host", async () => {
  const dialog = await runDialog();
  vi.mocked(invoke).mockClear();
  // Only the open pane, no new connection.
  fireEvent.click(dialog.getAllByRole("checkbox", { name: "web" })[0]);
  fireEvent.click(dialog.getByRole("button", { name: en["snippets.run"] }));
  expect(written()).toEqual([{ id: "s-1", text: "uptime\r" }]);
  expect(Object.keys(state.views)).toHaveLength(1);
});

test("a snippet on new connections opens a tab that types it after connecting", async () => {
  const dialog = await runDialog();
  fireEvent.click(dialog.getAllByRole("checkbox", { name: "web" })[1]);
  fireEvent.click(dialog.getByRole("button", { name: en["snippets.run"] }));
  expect(await screen.findByRole("tab", { name: /Snippet: Uptime/ })).toBeTruthy();
  const opened = Object.values(state.views).find((v) => v.pane.startup?.raw);
  expect(opened?.pane.startup?.raw).toBe("uptime\r");
});

test("Ctrl+Shift+S shows the terminal sidebar beside a session", async () => {
  fireEvent.doubleClick(await renderShell());
  await screen.findByRole("tab", { name: /web/ });
  expect(screen.queryByRole("complementary", { name: en["sidebar.title"] })).toBeNull();
  press("S");
  expect(await screen.findByRole("complementary", { name: en["sidebar.title"] })).toBeTruthy();
  press("S");
  expect(screen.queryByRole("complementary", { name: en["sidebar.title"] })).toBeNull();
});
