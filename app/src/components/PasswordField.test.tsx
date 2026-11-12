import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { PasswordChange } from "../api";
import { PasswordField } from "./PasswordField";

afterEach(cleanup);

let last: PasswordChange = { kind: "keep" };

function Harness({ stored }: { stored: boolean }) {
  const [value, setValue] = useState<PasswordChange>({ kind: "keep" });
  return (
    <PasswordField
      label="Password"
      stored={stored}
      value={value}
      onChange={(v) => {
        last = v;
        setValue(v);
      }}
    />
  );
}

test("stored, then change, gives set", () => {
  render(<Harness stored />);
  expect(screen.getByText("Stored")).toBeTruthy();
  fireEvent.click(screen.getByText("Change"));
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "new-secret" } });
  expect(last).toEqual({ kind: "set", secret: "new-secret" });
});

test("stored, then remove, gives clear and undo keeps", () => {
  render(<Harness stored />);
  fireEvent.click(screen.getByText("Remove"));
  expect(last).toEqual({ kind: "clear" });
  fireEvent.click(screen.getByText("Undo"));
  expect(last).toEqual({ kind: "keep" });
});

test("untouched stays keep, and an emptied new field is keep again", () => {
  last = { kind: "keep" };
  render(<Harness stored={false} />);
  expect(last).toEqual({ kind: "keep" });
  const input = screen.getByLabelText("Password");
  fireEvent.change(input, { target: { value: "x" } });
  expect(last).toEqual({ kind: "set", secret: "x" });
  fireEvent.change(input, { target: { value: "" } });
  expect(last).toEqual({ kind: "keep" });
});
