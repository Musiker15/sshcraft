import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { LogMark, LogMeta } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { LogViewer } from "./LogViewer";

const state = vi.hoisted(() => ({
  bytes: new Uint8Array(),
  calls: [] as { command: string; args: unknown }[],
  scrolled: [] as number[],
  found: [] as string[],
  viewport: 0,
  /** Lines the fake terminal reports as kept. */
  kept: 0,
  /** Marks the log has in Rust (a terminal bookmark may arrive there meanwhile). */
  stored: [] as LogMark[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { id: string; mark: LogMark | string }) => {
    state.calls.push({ command, args });
    if (command === "log_read") return state.bytes.buffer;
    if (command === "log_add_mark") {
      state.stored = [...state.stored, args!.mark as LogMark];
      return { ...meta(), marks: state.stored };
    }
    if (command === "log_remove_mark") {
      state.stored = state.stored.filter((m) => m.id !== args!.mark);
      return { ...meta(), marks: state.stored };
    }
    if (command === "log_export") return true;
    return null;
  }),
}));
vi.mock("../settings/SettingsProvider", () => ({
  useSettings: () => ({ settings: { font_family: "mono", font_size: 14, terminal_theme: "sshcraft-dark" } }),
}));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext(q: string) {
      state.found.push(q);
      return true;
    }
    findPrevious(q: string) {
      state.found.push(`back:${q}`);
      return true;
    }
  },
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    lines = 0;
    buffer = {
      active: {
        baseY: 0,
        get cursorY() {
          return 0;
        },
        get viewportY() {
          return state.viewport;
        },
        get length() {
          return 0;
        },
      },
    };
    constructor() {
      const self = this;
      this.buffer.active = {
        baseY: 0,
        get cursorY() {
          return self.lines;
        },
        get viewportY() {
          return state.viewport;
        },
        get length() {
          return state.kept || self.lines;
        },
      };
    }
    loadAddon() {}
    open() {}
    dispose() {}
    write(data: Uint8Array, done: () => void) {
      this.lines += data.filter((b) => b === 10).length;
      done();
    }
    scrollToLine(line: number) {
      state.scrolled.push(line);
    }
  },
}));

const encoder = new TextEncoder();
const mark = (id: string, offset: number, kind: LogMark["kind"], text: string): LogMark => ({
  id,
  offset,
  kind,
  text,
  created_at: 1,
});
function meta(): LogMeta {
  return {
    id: "log1",
    title: "web",
    kind: "ssh",
    host_id: "h1",
    started_at: Date.UTC(2026, 10, 13),
    ended_at: Date.UTC(2026, 10, 13, 1),
    size: state.bytes.length,
    truncated: false,
    marks: [mark("m1", 6, "bookmark", "after two"), mark("m2", 12, "comment", "end")],
  };
}

beforeEach(() => {
  // Six lines of two bytes each: "a\n" ... "f\n".
  state.bytes = encoder.encode("a\nb\nc\nd\ne\nf\n");
  state.calls = [];
  state.scrolled = [];
  state.found = [];
  state.viewport = 0;
  state.kept = 0;
  state.stored = meta().marks;
});
afterEach(cleanup);

async function show(query = "", log: LogMeta = meta()) {
  const onChanged = vi.fn();
  render(
    <I18nProvider language="en">
      <LogViewer log={log} query={query} onClose={() => {}} onChanged={onChanged} />
    </I18nProvider>,
  );
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_read")).toBe(true));
  await vi.waitFor(() => expect(screen.getByRole("button", { name: en["logs.export"] })).toBeTruthy());
  await new Promise((r) => setTimeout(r, 0));
  return onChanged;
}

test("a mark jumps to the line written up to it", async () => {
  await show();
  fireEvent.click(screen.getByText("after two"));
  expect(state.scrolled).toEqual([3]);
  fireEvent.click(screen.getByText("end"));
  expect(state.scrolled).toEqual([3, 6]);
});

test("a search from the logs page runs after loading", async () => {
  await show("nginx");
  expect(state.found).toEqual(["nginx"]);
  fireEvent.change(screen.getByLabelText(en["logs.find"]), { target: { value: "c" } });
  fireEvent.click(screen.getByRole("button", { name: en["logs.findPrev"] }));
  expect(state.found).toEqual(["nginx", "back:c"]);
});

test("a new comment goes at the top visible line and keeps marks set meanwhile", async () => {
  const onChanged = await show();
  // A bookmark from the terminal reaches Rust while the viewer is open.
  state.stored = [...state.stored, mark("t1", 2, "bookmark", "from terminal")];
  state.viewport = 4;
  fireEvent.click(screen.getByRole("button", { name: /Comment/ }));
  fireEvent.change(screen.getByLabelText(en["logs.comment"]), { target: { value: "deploy starts" } });
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(onChanged).toHaveBeenCalled());
  const added = (state.calls.find((c) => c.command === "log_add_mark")!.args as { mark: LogMark }).mark;
  expect(added.kind).toBe("comment");
  // Pieces end at the marks (bytes 6 and 12); line 4 lies after the one ending at byte 6.
  expect(added.offset).toBe(6);
  expect(await screen.findByText("from terminal")).toBeTruthy();
  expect(screen.getByText("deploy starts")).toBeTruthy();
});

test("a comment on a log still being recorded points into its output", async () => {
  await show("", { ...meta(), ended_at: null, size: 0, marks: [] });
  state.viewport = 6;
  fireEvent.click(screen.getByRole("button", { name: /Comment/ }));
  fireEvent.change(screen.getByLabelText(en["logs.comment"]), { target: { value: "live" } });
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_add_mark")).toBe(true));
  const added = (state.calls.find((c) => c.command === "log_add_mark")!.args as { mark: LogMark }).mark;
  expect(added.offset).toBeGreaterThan(0);
  expect(added.offset).toBeLessThanOrEqual(12);
});

test("a log longer than the viewer keeps says so and takes no marks", async () => {
  state.kept = 100_000;
  await show();
  expect(screen.getByText(/longer than/)).toBeTruthy();
  expect(screen.getByRole("button", { name: /Comment/ }).hasAttribute("disabled")).toBe(true);
  fireEvent.click(screen.getByText("after two"));
  expect(state.scrolled).toEqual([]);
});

test("an empty comment is not saved", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: /Comment/ }));
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  expect(state.calls.some((c) => c.command === "log_add_mark")).toBe(false);
});

test("removing a mark removes only that one", async () => {
  await show();
  fireEvent.click(screen.getAllByRole("button", { name: en["logs.removeMark"] })[0]);
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_remove_mark")).toBe(true));
  expect(state.calls.find((c) => c.command === "log_remove_mark")!.args).toEqual({ id: "log1", mark: "m1" });
  await vi.waitFor(() => expect(screen.queryByText("after two")).toBeNull());
});

test("export asks Rust for the format; the file dialog is the app's", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: /Export/ }));
  fireEvent.click(screen.getByRole("menuitem", { name: en["logs.exportText"] }));
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_export")).toBe(true));
  expect(state.calls.find((c) => c.command === "log_export")!.args).toEqual({ id: "log1", format: "text" });
});
