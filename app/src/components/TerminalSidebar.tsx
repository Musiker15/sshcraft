// Sidebar beside the terminal: snippets (click pastes, the button runs) and the focused pane's
// history (click pastes).
import { Play, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { historyList, snippetsList, type HistoryEntry, type Snippet } from "../api";
import { snippetInput } from "../forms/snippetRun";
import { useT } from "../i18n/i18n";

type Props = {
  /** History scope of the focused pane; `null` where there is none (serial). */
  scope: string | null;
  onInsert: (text: string) => void;
  onRun: (text: string) => void;
};

export function TerminalSidebar({ scope, onInsert, onRun }: Props) {
  const t = useT();
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");

  useEffect(() => {
    snippetsList().then((list) => setSnippets(list ?? []), () => setSnippets([]));
  }, []);
  useEffect(() => {
    if (!scope) {
      setHistory([]);
      return;
    }
    historyList(scope).then((list) => setHistory(list ?? []), () => setHistory([]));
  }, [scope]);

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
            <button className="sidebar-entry" title={s.content} onClick={() => onInsert(snippetInput(s.content, false))}>
              {s.name}
            </button>
            <button
              className="icon"
              aria-label={t("snippets.run")}
              title={t("snippets.run")}
              onClick={() => onRun(snippetInput(s.content, true))}
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
            <button className="sidebar-entry mono" onClick={() => onInsert(h.command)}>
              {h.command}
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}
