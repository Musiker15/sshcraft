import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { WorkspaceTemplate } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import type { SessionTab } from "../state/tabs";
import { TemplateDialog } from "./TemplateDialog";

const calls = vi.hoisted(() => ({ saved: [] as WorkspaceTemplate[] }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { template: WorkspaceTemplate }) => {
    if (command === "save_workspace_template") {
      calls.saved.push(args!.template);
      return args!.template;
    }
    return null;
  }),
}));

beforeEach(() => {
  calls.saved = [];
});
afterEach(cleanup);

const shell = { program: "pwsh.exe", args: [], label: "PowerShell" };
const tab: SessionTab = {
  kind: "session",
  id: "t",
  focusedId: "a",
  layout: "split",
  broadcast: false,
  colSizes: [2, 1],
  rowSizes: [1],
  panes: [
    { id: "a", target: { kind: "host", host_id: "h1" }, title: "web", status: { state: "connected" }, activity: "none", cwd: "/srv/app" },
    { id: "b", target: { kind: "local", shell, cwd: "C:\\x" }, title: "PowerShell", status: { state: "connected" }, activity: "none" },
  ],
};

function show(existing: WorkspaceTemplate[] = []) {
  const done = vi.fn();
  render(
    <I18nProvider language="en">
      <TemplateDialog tab={tab} existing={existing} onDone={done} onCancel={() => {}} />
    </I18nProvider>,
  );
  return done;
}

test("saves targets, titles, directories, commands, layout and sizes", async () => {
  const done = show();
  expect((screen.getByLabelText(en["templates.name"]) as HTMLInputElement).value).toBe("web · PowerShell");
  fireEvent.change(screen.getByLabelText(en["templates.name"]), { target: { value: "Ops" } });
  const commands = screen.getAllByLabelText(en["templates.command"]);
  fireEvent.change(commands[0], { target: { value: "htop" } });
  const dirs = screen.getAllByLabelText(en["templates.cwd"]);
  expect((dirs[0] as HTMLInputElement).value).toBe("/srv/app");
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(done).toHaveBeenCalled());
  expect(calls.saved[0]).toMatchObject({
    name: "Ops",
    layout: "split",
    col_sizes: [2, 1],
    row_sizes: [1],
    panes: [
      { target: { kind: "host", host_id: "h1" }, title: "web", cwd: "/srv/app", command: "htop" },
      { target: { kind: "local", shell }, title: "PowerShell", cwd: null, command: null },
    ],
  });
  expect(calls.saved[0].panes[1].target).toEqual({ kind: "local", shell });
});

test("the same name overwrites the template with that name", async () => {
  const existing: WorkspaceTemplate = { id: "keep-me", name: "Ops", layout: "focus", col_sizes: [], row_sizes: [], panes: [] };
  const done = show([existing]);
  fireEvent.change(screen.getByLabelText(en["templates.name"]), { target: { value: "Ops" } });
  expect(screen.getByText(en["templates.overwrites"])).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["common.save"] }));
  await vi.waitFor(() => expect(done).toHaveBeenCalled());
  expect(calls.saved[0].id).toBe("keep-me");
});
