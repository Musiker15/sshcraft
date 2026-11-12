import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { TagInput } from "./TagInput";

afterEach(cleanup);

function Harness({ start }: { start: string[] }) {
  const [tags, setTags] = useState(start);
  return (
    <I18nProvider language="en">
      <TagInput value={tags} onChange={setTags} />
      <output data-testid="tags">{tags.join("|")}</output>
    </I18nProvider>
  );
}

test("enter and comma add tags, duplicates do not", () => {
  render(<Harness start={["prod"]} />);
  const input = screen.getByLabelText(en["form.tags"]);
  fireEvent.change(input, { target: { value: "web" } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.change(input, { target: { value: "db," } });
  fireEvent.change(input, { target: { value: "PROD" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(screen.getByTestId("tags").textContent).toBe("prod|web|db");
});

test("a tag can be removed", () => {
  render(<Harness start={["prod", "web"]} />);
  fireEvent.click(screen.getByRole("button", { name: en["tags.remove"].replace("{tag}", "prod") }));
  expect(screen.getByTestId("tags").textContent).toBe("web");
});
