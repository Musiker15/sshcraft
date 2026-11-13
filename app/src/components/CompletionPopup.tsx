// The suggestions while typing, at the terminal cursor. Shown passively; keys reach it only once
// one is selected (see `keyAction`).
import { useT } from "../i18n/i18n";
import type { Suggestion } from "../terminal/completion";

type Props = { suggestions: Suggestion[]; selected: number | null; at: { left: number; top: number } };

export function CompletionPopup({ suggestions, selected, at }: Props) {
  const t = useT();
  if (suggestions.length === 0) return null;
  return (
    <ul className="completion" role="listbox" aria-label={t("completion.title")} style={{ left: at.left, top: at.top }}>
      {suggestions.map((s, index) => (
        <li
          key={`${s.source}:${s.label}`}
          role="option"
          aria-selected={index === selected}
          className={index === selected ? "completion-item active" : "completion-item"}
        >
          <span className="completion-label">{s.label}</span>
          <span className="completion-source">{t(`completion.source.${s.source}`)}</span>
        </li>
      ))}
    </ul>
  );
}
