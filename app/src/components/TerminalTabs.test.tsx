import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import en from "../i18n/en.json";
import { EndedLine } from "./TerminalTabs";

afterEach(cleanup);

test("an auth failure with a code shows the code's text, not the server message", () => {
  render(<EndedLine reason="auth_failed" code="agent_refused" detail={null} />);
  expect(screen.getByText(en["errors.agent_refused"])).toBeTruthy();
  expect(screen.queryByText(en["session.ended.auth_failed"])).toBeNull();
});

test("an auth failure without a code blames the server", () => {
  render(<EndedLine reason="auth_failed" code={null} detail={null} />);
  expect(screen.getByText(en["session.ended.auth_failed"])).toBeTruthy();
});

test("a failed first attempt says connection failed and shows the detail", () => {
  render(<EndedLine reason="connect_failed" code="network" detail="connection refused" />);
  expect(screen.getByText(new RegExp(en["session.ended.connect_failed"]))).toBeTruthy();
  expect(screen.getByText("connection refused")).toBeTruthy();
});
