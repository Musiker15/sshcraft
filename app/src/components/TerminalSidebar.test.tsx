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
  const onPaste = vi.fn();
  render(
    <I18nProvider language="en">
      <TerminalSidebar scope="host:x" onPaste={onPaste} />
    </I18nProvider>,
  );
  return { onPaste };
}

test("a snippet is pasted, or run with its button", async () => {
  const { onPaste } = show();
  fireEvent.click(await screen.findByText("Deploy"));
  expect(onPaste).toHaveBeenCalledWith("cd /srv\ngit pull", false);
  fireEvent.click(screen.getByRole("button", { name: en["snippets.run"] }));
  expect(onPaste).toHaveBeenCalledWith("cd /srv\ngit pull", true);
});

test("a command from the history is pasted", async () => {
  const { onPaste } = show();
  fireEvent.click(await screen.findByText("systemctl status nginx"));
  expect(onPaste).toHaveBeenCalledWith("systemctl status nginx", false);
});

test("search narrows both lists", async () => {
  show();
  await screen.findByText("Deploy");
  fireEvent.change(screen.getByLabelText(en["sidebar.search"]), { target: { value: "nginx" } });
  expect(screen.queryByText("Deploy")).toBeNull();
  expect(screen.getByText("systemctl status nginx")).toBeTruthy();
});

test("this terminal's history can be cleared", async () => {
  show();
  await screen.findByText("systemctl status nginx");
  fireEvent.click(screen.getByRole("button", { name: en["sidebar.clearHistory"] }));
  await vi.waitFor(() => expect(screen.queryByText("systemctl status nginx")).toBeNull());
});
