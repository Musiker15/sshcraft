// Loads the device settings once, applies language and theme, and saves changes through Rust.
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { getSettings, saveSettings, systemLocale, type AppSettings } from "../api";
import { I18nProvider } from "../i18n/i18n";
import { resolveLanguage } from "../i18n/language";
import { resolveAppTheme } from "./themes";

type SettingsContextValue = {
  settings: AppSettings;
  update: (patch: Partial<AppSettings>) => Promise<void>;
};

const SettingsContext = createContext<SettingsContextValue | null>(null);

export function useSettings(): SettingsContextValue {
  const value = useContext(SettingsContext);
  if (!value) throw new Error("useSettings needs a SettingsProvider");
  return value;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function usePrefersDark(): boolean {
  const query = typeof window.matchMedia === "function" ? window.matchMedia(DARK_QUERY) : null;
  const [dark, setDark] = useState(query?.matches ?? true);
  useEffect(() => {
    if (!query) return;
    const changed = (e: MediaQueryListEvent) => setDark(e.matches);
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, [query]);
  return dark;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [locale, setLocale] = useState<string | null>(null);
  const prefersDark = usePrefersDark();

  useEffect(() => {
    void Promise.all([getSettings(), systemLocale().catch(() => null)]).then(([s, l]) => {
      setLocale(l ?? navigator.language);
      setSettings(s);
    });
  }, []);

  const theme = settings ? resolveAppTheme(settings.app_theme, prefersDark) : "dark";
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const update = useCallback(
    async (patch: Partial<AppSettings>) => {
      if (!settings) return;
      const wanted = { ...settings, ...patch };
      setSettings(wanted);
      // Rust keeps values in range; show what was actually stored.
      setSettings(await saveSettings(wanted));
    },
    [settings],
  );

  if (!settings) return null;
  return (
    <SettingsContext.Provider value={{ settings, update }}>
      <I18nProvider language={resolveLanguage(settings.language, locale)}>{children}</I18nProvider>
    </SettingsContext.Provider>
  );
}
