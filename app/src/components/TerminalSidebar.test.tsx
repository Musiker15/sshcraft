import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { TerminalSidebar } from "./TerminalSidebar";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "snippets_list")
      return [{ id: "1", name: "Deploy", package: "", content: "cd /srv\ngit pull", default_targets: [] }];
    if (command === "history_list") return [{ command: "systemctl status nginx", uses: 2, last_used: 5 }];
    return null;
  }),
}));

afterEach(cleanup);

function show() {
  const onInsert = vi.fn();
  const onRun = vi.fn();
  render(
    <I18nProvider language="en">
      <TerminalSidebar scope="host:x" onInsert={onInsert} onRun={onRun} />
    </I18nProvider>,
  );
  return { onInsert, onRun };
}

test("a snippet is pasted, or run with its button", async () => {
  const { onInsert, onRun } = show();
  fireEvent.click(await screen.findByText("Deploy"));
  expect(onInsert).toHaveBeenCalledWith("cd /srv\rgit pull");
  fireEvent.click(screen.getByRole("button", { name: en["snippets.run"] }));
  expect(onRun).toHaveBeenCalledWith("cd /srv\rgit pull\r");
});

test("a command from the history is pasted", async () => {
  const { onInsert } = show();
  fireEvent.click(await screen.findByText("systemctl status nginx"));
  expect(onInsert).toHaveBeenCalledWith("systemctl status nginx");
});

test("search narrows both lists", async () => {
  show();
  await screen.findByText("Deploy");
  fireEvent.change(screen.getByLabelText(en["sidebar.search"]), { target: { value: "nginx" } });
  expect(screen.queryByText("Deploy")).toBeNull();
  expect(screen.getByText("systemctl status nginx")).toBeTruthy();
});
