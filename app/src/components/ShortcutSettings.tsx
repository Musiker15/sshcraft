// Settings block: every action with its shortcuts; "Change" records the next key combination.
import { useEffect, useState } from "react";
import { useT } from "../i18n/i18n";
import { useSettings } from "../settings/SettingsProvider";
import {
  DEFAULT_SHORTCUTS,
  SHORTCUT_ACTIONS,
  comboOf,
  findConflict,
  isTerminalSafe,
  isValidCombo,
  resolveBindings,
  type ShortcutAction,
} from "../shell/shortcuts";

type Problem = { action: ShortcutAction; text: string } | null;

export function ShortcutSettings() {
  const t = useT();
  const { settings, update } = useSettings();
  const bindings = resolveBindings(settings.shortcuts);
  const [recording, setRecording] = useState<ShortcutAction | null>(null);
  const [problem, setProblem] = useState<Problem>(null);

  const save = (action: ShortcutAction, combos: string[] | null) => {
    const shortcuts = { ...settings.shortcuts };
    if (combos === null) delete shortcuts[action];
    else shortcuts[action] = combos;
    void update({ shortcuts });
  };

  // While recording, the next key press is the new combination; nothing else sees it.
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey) {
        setRecording(null);
        return;
      }
      const combo = comboOf(e);
      if (combo === null) return;
      setRecording(null);
      if (!isValidCombo(combo)) {
        setProblem({ action: recording, text: t("shortcuts.invalid") });
        return;
      }
      const other = findConflict(bindings, recording, combo);
      if (other) {
        setProblem({ action: recording, text: t("shortcuts.conflict", { action: t(`shortcuts.action.${other}`) }) });
        return;
      }
      setProblem(null);
      save(recording, [combo]);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  return (
    <div className="settings-block">
      <h2 className="group-title">{t("shortcuts.title")}</h2>
      <p className="hint">{t("shortcuts.hint")}</p>
      <ul className="shortcut-list">
        {SHORTCUT_ACTIONS.map((action) => {
          const custom = action in settings.shortcuts;
          return (
            <li key={action} className="shortcut-row">
              <span className="shortcut-name">{t(`shortcuts.action.${action}`)}</span>
              <span className="shortcut-combos">
                {bindings[action].length === 0 && <span className="hint">{t("shortcuts.none")}</span>}
                {bindings[action].map((combo) => (
                  <span key={combo} className="shortcut-combo">
                    <kbd>{combo}</kbd>
                    {!isTerminalSafe(combo) && <span className="hint">{t("shortcuts.appOnly")}</span>}
                  </span>
                ))}
              </span>
              <span className="shortcut-actions">
                <button
                  className="link"
                  onClick={() => {
                    setProblem(null);
                    setRecording(action);
                  }}
                >
                  {recording === action ? t("shortcuts.recording") : t("shortcuts.change")}
                </button>
                {bindings[action].length > 0 && (
                  <button className="link" onClick={() => save(action, [])}>
                    {t("shortcuts.unbind")}
                  </button>
                )}
                {custom && (
                  <button className="link" onClick={() => save(action, null)} title={DEFAULT_SHORTCUTS[action].join(", ")}>
                    {t("shortcuts.reset")}
                  </button>
                )}
              </span>
              {problem?.action === action && (
                <span className="error" role="alert">
                  {problem.text}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
