import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../../i18n/en.json";
import { I18nProvider } from "../../i18n/i18n";
import { AccountKeyDialog } from "./AccountKeyDialog";

afterEach(cleanup);

const created = { server_id: "s1", account_key: "sshcraft-account-1:ABCDEF", qr_svg: "<svg></svg>" };

test("closing needs the confirmation that the key was saved", () => {
  const onClose = vi.fn();
  render(
    <I18nProvider language="en">
      <AccountKeyDialog created={created} onClose={onClose} />
    </I18nProvider>,
  );
  expect(screen.getByText(created.account_key)).toBeTruthy();
  const close = screen.getByRole("button", { name: en["sync.keySavedClose"] }) as HTMLButtonElement;
  expect(close.disabled).toBe(true);
  fireEvent.click(screen.getByLabelText(en["sync.keySaved"]));
  expect(close.disabled).toBe(false);
  fireEvent.click(close);
  expect(onClose).toHaveBeenCalled();
});

test("copy puts the key on the clipboard", async () => {
  const writeText = vi.fn(async () => {});
  Object.assign(navigator, { clipboard: { writeText } });
  render(
    <I18nProvider language="en">
      <AccountKeyDialog created={created} onClose={() => {}} />
    </I18nProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: en["sync.copy"] }));
  expect(writeText).toHaveBeenCalledWith(created.account_key);
});
