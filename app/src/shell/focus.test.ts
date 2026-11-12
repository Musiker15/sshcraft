import { expect, test, vi } from "vitest";
import { syncTerminalFocus } from "./focus";

test("the active terminal takes the keyboard, a hidden one gives it up", () => {
  const terminal = { focus: vi.fn(), blur: vi.fn() };
  syncTerminalFocus(terminal, true);
  expect(terminal.focus).toHaveBeenCalledTimes(1);
  syncTerminalFocus(terminal, false);
  expect(terminal.blur).toHaveBeenCalledTimes(1);
  expect(terminal.focus).toHaveBeenCalledTimes(1);
});
