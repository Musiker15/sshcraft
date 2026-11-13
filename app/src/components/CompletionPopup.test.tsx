import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { I18nProvider } from "../i18n/i18n";
import { CompletionPopup } from "./CompletionPopup";

afterEach(cleanup);

test("lists the suggestions and marks the selected one", () => {
  render(
    <I18nProvider language="en">
      <CompletionPopup
        suggestions={[
          { label: "git status", insert: "t status", replace: false, source: "history" },
          { label: "git", insert: "t", replace: false, source: "command" },
        ]}
        selected={1}
        at={{ left: 10, top: 20 }}
      />
    </I18nProvider>,
  );
  const options = screen.getAllByRole("option");
  expect(options.map((o) => o.textContent?.includes("git"))).toEqual([true, true]);
  expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "true"]);
});
