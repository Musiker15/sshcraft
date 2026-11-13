// Sidebar beside the terminal: snippets (click pastes, the button runs) and the focused pane's
// history (click pastes, can be cleared).
import { Play, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { historyClear, historyList, snippetsList, type HistoryEntry, type Snippet } from "../api";
import { useT } from "../i18n/i18n";
import { HISTORY_CLEARED, SNIPPETS_CHANGED } from "../terminal/panes";

type Props = {
  /** History scope of the focused pane; `null` where there is none (serial). */
  scope: string | null;
  /** Pastes into the focused pane; `run` presses Enter after it. */
  onPaste: (content: string, run: boolean) => void;
};

export function TerminalSidebar({ scope, onPaste }: Props) {
  const t = useT();
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const load = () => snippetsList().then((list) => setSnippets(list ?? []), () => setSnippets([]));
    load();
    window.addEventListener(SNIPPETS_CHANGED, load);
    return () => window.removeEventListener(SNIPPETS_CHANGED, load);
  }, []);
  useEffect(() => {
    if (!scope) {
      setHistory([]);
      return;
    }
    historyList(scope).then((list) => setHistory(list ?? []), () => setHistory([]));
    // Cleared here or in the settings: the list goes at once.
    const cleared = (e: Event) => {
      const which = (e as CustomEvent<string | null>).detail;
      if (which === null || which === scope) setHistory([]);
    };
    window.addEventListener(HISTORY_CLEARED, cleared);
    return () => window.removeEventListener(HISTORY_CLEARED, cleared);
  }, [scope]);

  const clear = () => {
    if (!scope) return;
    void historyClear(scope).then(() => {
      setHistory([]);
      window.dispatchEvent(new CustomEvent(HISTORY_CLEARED, { detail: scope }));
    }, () => {});
  };

  const q = query.trim().toLocaleLowerCase();
  const matches = (text: string) => q === "" || text.toLocaleLowerCase().includes(q);
  const shownSnippets = snippets.filter((s) => matches(s.name) || matches(s.content));
  const shownHistory = history.filter((h) => matches(h.command));

  return (
    <aside className="terminal-sidebar" aria-label={t("sidebar.title")}>
      <label className="search">
        <Search size={14} aria-hidden="true" />
        <input
          type="search"
          value={query}
          placeholder={t("sidebar.search")}
          aria-label={t("sidebar.search")}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>
      <h3 className="group-title">{t("nav.snippets")}</h3>
      {shownSnippets.length === 0 && <p className="hint">{t("sidebar.noSnippets")}</p>}
      <ul className="sidebar-list">
        {shownSnippets.map((s) => (
          <li key={s.id} className="sidebar-row">
            <button className="sidebar-entry" title={s.content} onClick={() => onPaste(s.content, false)}>
              {s.name}
            </button>
            <button
              className="icon"
              aria-label={t("snippets.run")}
              title={t("snippets.run")}
              onClick={() => onPaste(s.content, true)}
            >
              <Play size={12} />
            </button>
          </li>
        ))}
      </ul>
      <h3 className="group-title">{t("sidebar.history")}</h3>
      {shownHistory.length === 0 && <p className="hint">{t("sidebar.noHistory")}</p>}
      <ul className="sidebar-list">
        {shownHistory.map((h) => (
          <li key={h.command} className="sidebar-row">
            <button className="sidebar-entry mono" onClick={() => onPaste(h.command, false)}>
              {h.command}
            </button>
          </li>
        ))}
      </ul>
      {scope && history.length > 0 && (
        <button className="link" onClick={clear}>
          {t("sidebar.clearHistory")}
        </button>
      )}
    </aside>
  );
}
