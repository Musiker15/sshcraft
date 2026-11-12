import { expect, test } from "vitest";
import de from "./de.json";
import en from "./en.json";
import { errorDetail, errorText } from "./errors";
import { translate, translateWith, type Language } from "./i18n";
import { resolveLanguage, type LanguageSetting } from "./language";

const enMessages: Record<string, string> = en;
const deMessages: Record<string, string> = de;
const t = (lang: Language) => (key: string, params?: Record<string, string | number>) => translate(lang, key, params);
const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test("en and de have the same keys", () => {
  expect(Object.keys(deMessages).sort()).toEqual(Object.keys(enMessages).sort());
});

test("placeholders match per key", () => {
  for (const key of Object.keys(enMessages)) {
    expect([key, placeholders(deMessages[key] ?? "")]).toEqual([key, placeholders(enMessages[key])]);
  }
});

test("no empty values and no dashes", () => {
  for (const value of [...Object.values(enMessages), ...Object.values(deMessages)]) {
    expect(value.trim()).not.toBe("");
    expect(value).not.toMatch(/[—–]/);
  }
});

test.each([
  ["auto", "de-DE", "de"],
  ["auto", "de_AT.UTF-8", "de"],
  ["auto", "DE", "de"],
  ["auto", "fr-FR", "en"],
  ["auto", "C", "en"],
  ["auto", "", "en"],
  ["auto", null, "en"],
  ["en", "de-DE", "en"],
  ["de", "en-US", "de"],
])("resolveLanguage(%s, %s) is %s", (setting, locale, want) => {
  expect(resolveLanguage(setting as LanguageSetting, locale)).toBe(want);
});

test("params are filled in", () => {
  expect(translate("en", "hosts.deleteConfirm", { name: "web" })).toContain("web");
});

test("plural forms follow the language", () => {
  expect(translate("en", "hosts.count", { count: 1 })).toBe("1 host");
  expect(translate("en", "hosts.count", { count: 2 })).toBe("2 hosts");
  expect(translate("de", "hosts.count", { count: 1 })).toBe("1 Host");
  expect(translate("de", "hosts.count", { count: 0 })).toBe("0 Hosts");
});

test("missing de key falls back to en", () => {
  expect(translateWith({}, { a: "A" }, "de", "a")).toBe("A");
});

test("a key missing everywhere comes back as the key", () => {
  expect(translateWith({}, {}, "en", "nope.missing")).toBe("nope.missing");
});

test("known code is translated", () => {
  expect(errorText(t("en"), { code: "agent_unavailable", detail: null })).toBe(enMessages["errors.agent_unavailable"]);
  expect(errorText(t("de"), { code: "agent_unavailable", detail: null })).toBe(deMessages["errors.agent_unavailable"]);
});

test("unknown code and plain strings use the generic text", () => {
  expect(errorText(t("en"), { code: "future_code", detail: "x" })).toBe(enMessages["errors.unknown"]);
  expect(errorText(t("en"), "boom")).toBe(enMessages["errors.unknown"]);
  expect(errorDetail("boom")).toBe("boom");
  expect(errorDetail({ code: "future_code", detail: "x" })).toBe("x");
  expect(errorDetail({ code: "agent_unavailable", detail: null })).toBeNull();
});
