import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { AuthPromptDialog } from "./AuthPromptDialog";

afterEach(cleanup);

const prompt = {
  prompt_id: 7,
  host_name: "web",
  username: "root",
  name: "",
  instructions: "",
  prompts: [
    { text: "Password: ", echo: false },
    { text: "Code: ", echo: true },
  ],
};

test("a hidden and a visible line, answers in order", () => {
  const onAnswer = vi.fn();
  render(<AuthPromptDialog prompt={prompt} onAnswer={onAnswer} />);
  const password = screen.getByLabelText("Password:") as HTMLInputElement;
  const code = screen.getByLabelText("Code:") as HTMLInputElement;
  expect(password.type).toBe("password");
  expect(code.type).toBe("text");
  fireEvent.change(password, { target: { value: "pw" } });
  fireEvent.change(code, { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: en["auth.submit"] }));
  expect(onAnswer).toHaveBeenCalledWith(["pw", "123456"]);
});

test("cancel answers null", () => {
  const onAnswer = vi.fn();
  render(<AuthPromptDialog prompt={prompt} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: en["common.cancel"] }));
  expect(onAnswer).toHaveBeenCalledWith(null);
});
