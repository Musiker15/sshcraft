import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { ImportDialog } from "./ImportDialog";

const calls = vi.hoisted(() => ({ log: [] as { command: string; args: unknown }[] }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null), save: vi.fn(async () => null) }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: unknown) => {
    calls.log.push({ command, args });
    if (command === "import_defaults")
      return { known_hosts: null, ssh_dir: null, mobaxterm: null, securecrt: null, putty_registry: false };
    if (command === "import_preview")
      return {
        host_keys: null,
        plan: {
          skipped: ["row 4: no address"],
          candidates: [
            {
              name: "web",
              group_path: ["Kunden"],
              settings: { address: "web.example.org", port: null, username: "deploy", key: null },
              key_note: null,
              warnings: [],
            },
            {
              name: "odd",
              group_path: [],
              settings: { address: "odd.example.org", port: null, username: null, key: null },
              key_note: null,
              warnings: [{ kind: "bad_port" }],
            },
          ],
        },
      };
    if (command === "import_apply") return { created: 1, duplicates: 0, groups: [], hosts: [] };
    return null;
  }),
}));

afterEach(() => {
  cleanup();
  calls.log = [];
});

test("preview, warnings start unchecked, only checked ones are imported", async () => {
  const onDone = vi.fn();
  render(
    <I18nProvider language="en">
      <ImportDialog servers={[]} initial="csv" onDone={onDone} onCancel={() => {}} />
    </I18nProvider>,
  );
  fireEvent.change(screen.getByLabelText(en["import.path"]), { target: { value: "C:\\hosts.csv" } });
  fireEvent.click(screen.getByRole("button", { name: en["import.preview"] }));
  expect(await screen.findByText(en["import.warning.bad_port"])).toBeTruthy();
  expect(screen.getByText("row 4: no address")).toBeTruthy();
  const odd = screen.getByLabelText(/odd/) as HTMLInputElement;
  expect(odd.checked).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: en["import.import"] }));
  expect(await screen.findByText(en["import.created.one"].replace("{count}", "1"))).toBeTruthy();
  const applied = calls.log.find((c) => c.command === "import_apply")!.args as {
    candidates: { name: string }[];
    location: string | null;
  };
  expect(applied.candidates.map((c) => c.name)).toEqual(["web"]);
  expect(applied.location).toBeNull();
});
