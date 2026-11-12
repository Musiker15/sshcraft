import { describe, expect, test } from "vitest";
import { fromConnectionSettings, toConnectionSettings, type SettingsFormValues } from "./hostForm";

const empty: SettingsFormValues = { address: "", port: "", username: "", keyChoice: "" };

describe("toConnectionSettings", () => {
  test("empty fields become null so the group value is inherited", () => {
    expect(toConnectionSettings(empty)).toEqual({ address: null, port: null, username: null, key: null });
  });

  test("whitespace-only fields count as empty", () => {
    expect(toConnectionSettings({ ...empty, address: "   " }).address).toBeNull();
  });

  test("filled fields are kept and trimmed", () => {
    const s = toConnectionSettings({ address: " example.org ", port: "2222", username: "root", keyChoice: "stored:k1" });
    expect(s).toEqual({ address: "example.org", port: 2222, username: "root", key: { kind: "stored", id: "k1" } });
  });

  test("port outside 1..65535 is rejected", () => {
    expect(codeOf(() => toConnectionSettings({ ...empty, port: "0" }))).toBe("port_range");
    expect(codeOf(() => toConnectionSettings({ ...empty, port: "70000" }))).toBe("port_range");
    expect(codeOf(() => toConnectionSettings({ ...empty, port: "22a" }))).toBe("port_not_number");
  });

  test("agent choice maps to an agent key ref", () => {
    expect(toConnectionSettings({ ...empty, keyChoice: "agent:SHA256:abc" }).key).toEqual({
      kind: "agent",
      fingerprint: "SHA256:abc",
    });
  });

  test("stored choice maps to a stored key ref", () => {
    expect(toConnectionSettings({ ...empty, keyChoice: "stored:k1" }).key).toEqual({ kind: "stored", id: "k1" });
  });

  test("an unknown choice is rejected", () => {
    expect(codeOf(() => toConnectionSettings({ ...empty, keyChoice: "bogus" }))).toBe("unknown_key_choice");
  });
});

test("fromConnectionSettings round-trips", () => {
  const values = { address: "a", port: "22", username: "u", keyChoice: "stored:k" };
  expect(fromConnectionSettings(toConnectionSettings(values))).toEqual(values);
  expect(fromConnectionSettings(toConnectionSettings(empty))).toEqual(empty);
});

test("fingerprints containing colons survive the round trip", () => {
  const values = { ...empty, keyChoice: "agent:SHA256:a:b/c+d" };
  expect(fromConnectionSettings(toConnectionSettings(values))).toEqual(values);
});

/** The translation code of what `run` throws, or null if it does not throw. */
function codeOf(run: () => unknown): string | null {
  try {
    run();
    return null;
  } catch (e) {
    return (e as { code?: string }).code ?? "no code";
  }
}
