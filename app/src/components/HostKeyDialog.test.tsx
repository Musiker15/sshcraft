import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { HostKeyDialog } from "./HostKeyDialog";

const prompt = { prompt_id: 1, host_name: "srv", address: "example.org", port: 22, fingerprint: "SHA256:abc" };

afterEach(cleanup);

test("the trust button is not focused, so a stray Enter cannot accept the key", () => {
  render(<HostKeyDialog prompt={prompt} onAnswer={() => {}} />);
  const trust = screen.getByRole("button", { name: en["hostKey.trust"] });
  expect(document.activeElement).not.toBe(trust);
});

test("the reject button has the focus", () => {
  render(<HostKeyDialog prompt={prompt} onAnswer={() => {}} />);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: en["hostKey.reject"] }));
});

test("answers reach the caller", () => {
  const onAnswer = vi.fn();
  render(<HostKeyDialog prompt={prompt} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: en["hostKey.trust"] }));
  expect(onAnswer).toHaveBeenCalledWith(true);
});

test("the dialog names host and address", () => {
  render(<HostKeyDialog prompt={prompt} onAnswer={() => {}} />);
  expect(screen.getByText(/srv/)).toBeTruthy();
  expect(screen.getByText(/example\.org:22/)).toBeTruthy();
});
