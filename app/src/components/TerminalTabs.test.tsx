import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { EndedLine } from "./TerminalTabs";

afterEach(cleanup);

test("an auth failure with a detail shows the detail, not the server message", () => {
  render(<EndedLine reason="auth_failed" detail="Proton Pass hat die Signatur abgelehnt." />);
  expect(screen.getByText(/Proton Pass hat die Signatur abgelehnt/)).toBeTruthy();
  expect(screen.queryByText(/Der Server hat den Schlüssel nicht angenommen/)).toBeNull();
});

test("an auth failure without a detail blames the server", () => {
  render(<EndedLine reason="auth_failed" detail={null} />);
  expect(screen.getByText(/Der Server hat den Schlüssel nicht angenommen/)).toBeTruthy();
});
