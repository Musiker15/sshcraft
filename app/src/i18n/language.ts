export type Language = "en" | "de";
export type LanguageSetting = "auto" | Language;

/** AUTO follows the operating system; anything that is not German falls back to English. */
export function resolveLanguage(setting: LanguageSetting, systemLocale: string | null): Language {
  if (setting !== "auto") return setting;
  return (systemLocale ?? "").slice(0, 2).toLowerCase() === "de" ? "de" : "en";
}
