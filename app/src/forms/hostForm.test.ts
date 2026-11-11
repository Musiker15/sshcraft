import { describe, expect, test } from "vitest";
import { fromConnectionSettings, toConnectionSettings, type SettingsFormValues } from "./hostForm";

const empty: SettingsFormValues = { address: "", port: "", username: "", keyId: "" };

describe("toConnectionSettings", () => {
  test("empty fields become null so the group value is inherited", () => {
    expect(toConnectionSettings(empty)).toEqual({ address: null, port: null, username: null, key: null });
  });

  test("whitespace-only fields count as empty", () => {
    expect(toConnectionSettings({ ...empty, address: "   " }).address).toBeNull();
  });

  test("filled fields are kept and trimmed", () => {
    const s = toConnectionSettings({ address: " example.org ", port: "2222", username: "root", keyId: "k1" });
    expect(s).toEqual({ address: "example.org", port: 2222, username: "root", key: { kind: "stored", id: "k1" } });
  });

  test("port outside 1..65535 is rejected", () => {
    expect(() => toConnectionSettings({ ...empty, port: "0" })).toThrow();
    expect(() => toConnectionSettings({ ...empty, port: "70000" })).toThrow();
    expect(() => toConnectionSettings({ ...empty, port: "22a" })).toThrow();
  });
});

test("fromConnectionSettings round-trips", () => {
  const values = { address: "a", port: "22", username: "u", keyId: "k" };
  expect(fromConnectionSettings(toConnectionSettings(values))).toEqual(values);
  expect(fromConnectionSettings(toConnectionSettings(empty))).toEqual(empty);
});
