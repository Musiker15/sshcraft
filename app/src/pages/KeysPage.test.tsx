import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { KeyInfo, ServerView } from "../api";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { KeysPage } from "./KeysPage";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));

afterEach(cleanup);

const key: KeyInfo = { id: "k1", name: "deploy", algorithm: "ssh-ed25519", fingerprint: "SHA256:x", public_key: "" };
const server = (id: string, name: string, syncKeys: boolean): ServerView => ({
  id,
  name,
  origin: "https://sync.example.org",
  sync_enabled: true,
  sync_keys: syncKeys,
  revoked: false,
  status: { state: "idle" },
});

test("a key can move only to servers that sync keys", () => {
  const onMove = vi.fn();
  render(
    <I18nProvider language="en">
      <KeysPage
        keys={[key]}
        onChanged={() => {}}
        servers={[server("s1", "home", true), server("s2", "work", false)]}
        onMove={onMove}
      />
    </I18nProvider>,
  );
  fireEvent.contextMenu(screen.getByText("deploy"));
  const home = en["sync.moveTo"].replace("{place}", "home");
  const work = en["sync.moveTo"].replace("{place}", "work");
  expect(screen.queryByText(work)).toBeNull();
  fireEvent.click(screen.getByText(home));
  expect(onMove).toHaveBeenCalledWith("k1", "s1");
});

test("a synced key shows its server and can go back to this device", () => {
  const onMove = vi.fn();
  render(
    <I18nProvider language="en">
      <KeysPage keys={[{ ...key, location: "s1" }]} onChanged={() => {}} servers={[server("s1", "home", true)]} onMove={onMove} />
    </I18nProvider>,
  );
  expect(screen.getByText("home")).toBeTruthy();
  fireEvent.contextMenu(screen.getByText("deploy"));
  fireEvent.click(screen.getByText(en["sync.moveTo"].replace("{place}", en["sync.localOnly"])));
  expect(onMove).toHaveBeenCalledWith("k1", null);
});

test("identities are listed and can be deleted", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  render(
    <I18nProvider language="en">
      <KeysPage
        keys={[key]}
        onChanged={() => {}}
        identities={[
          { id: "i1", name: "ops", username: "deploy", key: null, location: null, has_password: true },
        ]}
      />
    </I18nProvider>,
  );
  expect(screen.getByText("ops")).toBeTruthy();
  expect(screen.getByText(`deploy · ${en["identity.hasPassword"]}`)).toBeTruthy();
  const card = screen.getByText("ops").closest(".card") as HTMLElement;
  fireEvent.click(card.querySelector("button.danger") as HTMLElement);
  const confirm = screen.getAllByRole("button", { name: en["common.delete"] }).pop() as HTMLElement;
  fireEvent.click(confirm);
  expect(invoke).toHaveBeenCalledWith("identity_delete", { id: "i1" });
});
