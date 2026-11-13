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
  savePath: "C:\\logs\\web.txt" as string | null,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { id: string; marks: LogMark[] }) => {
    state.calls.push({ command, args });
    if (command === "log_read") return state.bytes.buffer;
    if (command === "log_save_marks") return { ...meta(), marks: args!.marks };
    return null;
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(async () => state.savePath) }));
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
  state.savePath = "C:\\logs\\web.txt";
});
afterEach(cleanup);

async function show(query = "") {
  const onChanged = vi.fn();
  render(
    <I18nProvider language="en">
      <LogViewer log={meta()} query={query} onClose={() => {}} onChanged={onChanged} />
    </I18nProvider>,
  );
  await vi.waitFor(() => expect(screen.getByRole("button", { name: /Bookmark/ }).hasAttribute("disabled")).toBe(false));
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

test("a new comment goes at the top visible line", async () => {
  const onChanged = await show();
  state.viewport = 4;
  fireEvent.click(screen.getByRole("button", { name: /Comment/ }));
  fireEvent.change(screen.getByLabelText(en["logs.comment"]), { target: { value: "deploy starts" } });
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(onChanged).toHaveBeenCalled());
  const saved = state.calls.find((c) => c.command === "log_save_marks")!.args as { marks: LogMark[] };
  const added = saved.marks.find((m) => m.text === "deploy starts")!;
  expect(added.kind).toBe("comment");
  // Pieces end at the marks (bytes 6 and 12); line 4 lies after the one ending at byte 6.
  expect(added.offset).toBe(6);
  expect(saved.marks).toHaveLength(3);
});

test("an empty comment is not saved", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: /Comment/ }));
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  expect(state.calls.some((c) => c.command === "log_save_marks")).toBe(false);
});

test("removing a mark saves the others", async () => {
  await show();
  fireEvent.click(screen.getAllByRole("button", { name: en["logs.removeMark"] })[0]);
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_save_marks")).toBe(true));
  const saved = state.calls.find((c) => c.command === "log_save_marks")!.args as { marks: LogMark[] };
  expect(saved.marks.map((m) => m.id)).toEqual(["m2"]);
});

test("export asks for the format and the file", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: /Export/ }));
  fireEvent.click(screen.getByRole("menuitem", { name: en["logs.exportText"] }));
  await vi.waitFor(() => expect(state.calls.some((c) => c.command === "log_export")).toBe(true));
  expect(state.calls.find((c) => c.command === "log_export")!.args).toEqual({
    id: "log1",
    path: "C:\\logs\\web.txt",
    format: "text",
  });
});

test("a cancelled file dialog exports nothing", async () => {
  state.savePath = null;
  await show();
  fireEvent.click(screen.getByRole("button", { name: /Export/ }));
  fireEvent.click(screen.getByRole("menuitem", { name: en["logs.exportRaw"] }));
  await new Promise((r) => setTimeout(r, 20));
  expect(state.calls.some((c) => c.command === "log_export")).toBe(false);
});
