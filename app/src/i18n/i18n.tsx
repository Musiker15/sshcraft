// Translation layer: flat JSON files per language, English is the fallback.
import { createContext, useCallback, useContext, type ReactNode } from "react";
import de from "./de.json";
import en from "./en.json";
import type { Language } from "./language";

export type { Language } from "./language";
export type Messages = Record<string, string>;
export type Params = Record<string, string | number>;
export type Translate = (key: string, params?: Params) => string;

const catalogs: Record<Language, Messages> = { en, de };

function lookup(primary: Messages, fallback: Messages, key: string): string | undefined {
  return primary[key] ?? fallback[key];
}

/** Pure lookup behind `translate`: plural forms, then fallback language, then the key itself. */
export function translateWith(
  primary: Messages,
  fallback: Messages,
  lang: Language,
  key: string,
  params?: Params,
): string {
  let text: string | undefined;
  if (typeof params?.count === "number") {
    const form = new Intl.PluralRules(lang).select(params.count);
    text = lookup(primary, fallback, `${key}.${form}`) ?? lookup(primary, fallback, `${key}.other`);
  }
  text ??= lookup(primary, fallback, key) ?? key;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}

export function translate(lang: Language, key: string, params?: Params): string {
  return translateWith(catalogs[lang], catalogs.en, lang, key, params);
}

const LanguageContext = createContext<Language>("en");

export function I18nProvider({ language, children }: { language: Language; children: ReactNode }) {
  return <LanguageContext.Provider value={language}>{children}</LanguageContext.Provider>;
}

export function useLanguage(): Language {
  return useContext(LanguageContext);
}

export function useT(): Translate {
  const lang = useLanguage();
  return useCallback((key: string, params?: Params) => translate(lang, key, params), [lang]);
}
