import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import type { AppSettings, AppTheme } from "../api";
import { useT } from "../i18n/i18n";
import { useSettings } from "../settings/SettingsProvider";
import { FONT_PRESETS, TERMINAL_THEMES, terminalTheme } from "../settings/themes";

const LANGUAGES: { value: AppSettings["language"]; name: string | null }[] = [
  { value: "auto", name: null },
  // Language names are written in their own language, whatever the UI language is.
  { value: "en", name: "English" },
  { value: "de", name: "Deutsch" },
];
const APP_THEMES: AppTheme[] = ["system", "dark", "light"];
/** Sample shell prompt for the colour preview; not a translatable text. */
const SAMPLE_PROMPT = "user@host:~$";

/** Language, look and terminal font of this device; every change is saved at once. */
export function SettingsPage() {
  const t = useT();
  const { settings, update } = useSettings();
  const [version, setVersion] = useState<string | null>(null);
  const [fontFamily, setFontFamily] = useState(settings.font_family);

  useEffect(() => {
    getVersion().then(setVersion, () => setVersion(null));
  }, []);
  useEffect(() => setFontFamily(settings.font_family), [settings.font_family]);
  const preview = terminalTheme(settings.terminal_theme).colors;

  return (
    <section className="page settings">
      <header className="page-head">
        <h1>{t("nav.settings")}</h1>
      </header>

      <div className="settings-block">
        <h2 className="group-title">{t("settings.general")}</h2>
        <label>
          {t("settings.language")}
          <select
            value={settings.language}
            onChange={(e) => void update({ language: e.target.value as AppSettings["language"] })}
          >
            {LANGUAGES.map((l) => (
              <option key={l.value} value={l.value}>
                {l.name ?? t("settings.language.auto")}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="settings-block">
        <h2 className="group-title">{t("settings.appearance")}</h2>
        <label>
          {t("settings.appTheme")}
          <select value={settings.app_theme} onChange={(e) => void update({ app_theme: e.target.value as AppTheme })}>
            {APP_THEMES.map((theme) => (
              <option key={theme} value={theme}>
                {t(`settings.appTheme.${theme}`)}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("settings.terminalTheme")}
          <select value={settings.terminal_theme} onChange={(e) => void update({ terminal_theme: e.target.value })}>
            {TERMINAL_THEMES.map((theme) => (
              <option key={theme.id} value={theme.id}>
                {theme.name}
              </option>
            ))}
          </select>
        </label>
        <div className="theme-preview" aria-hidden="true" style={{ background: preview.background, color: preview.foreground }}>
          <span>{SAMPLE_PROMPT}</span>
          {[preview.red, preview.green, preview.yellow, preview.blue].map((color) => (
            <span key={color} className="swatch" style={{ background: color }} />
          ))}
        </div>
        <label>
          {t("settings.fontFamily")}
          <input
            list="font-presets"
            value={fontFamily}
            onChange={(e) => setFontFamily(e.target.value)}
            onBlur={() => fontFamily !== settings.font_family && void update({ font_family: fontFamily })}
          />
          <datalist id="font-presets">
            {FONT_PRESETS.map((font) => (
              <option key={font} value={font} />
            ))}
          </datalist>
        </label>
        <label>
          {t("settings.fontSize")}
          <input
            type="number"
            min={8}
            max={32}
            value={settings.font_size}
            onChange={(e) => {
              const size = Number(e.target.value);
              if (Number.isFinite(size) && e.target.value !== "") void update({ font_size: size });
            }}
          />
        </label>
      </div>

      <div className="settings-block">
        <h2 className="group-title">{t("settings.about")}</h2>
        <p className="hint">{t("settings.version", { version: version ?? "?" })}</p>
        <p className="hint">{t("settings.license")}</p>
      </div>
    </section>
  );
}
