// Snippets: packages on the left, the snippets of the chosen one with search, edit and run.
import { Play, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { snippetDelete, snippetsList, type Host, type Snippet } from "../api";
import { SnippetForm } from "../components/SnippetForm";
import { useLanguage, useT } from "../i18n/i18n";
import { ConfirmDialog } from "../shell/ConfirmDialog";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";
import { Drawer } from "../shell/Drawer";
import { ErrorMessage } from "../shell/ErrorMessage";

type Props = { hosts: Host[]; onRun: (snippet: Snippet) => void };
/** `null`: all; `""`: without a package; else a package name. */
type Filter = string | null;

export function SnippetsPage({ hosts, onRun }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [filter, setFilter] = useState<Filter>(null);
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<{ snippet?: Snippet } | null>(null);
  const [confirm, setConfirm] = useState<Snippet | null>(null);
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; items: MenuItem[] } | null>(null);
  const [problem, setProblem] = useState<unknown>(null);

  const reload = useCallback(() => {
    snippetsList().then((list) => setSnippets(list ?? []), setProblem);
  }, []);
  useEffect(reload, [reload]);

  const packages = useMemo(
    () => [...new Set(snippets.map((s) => s.package).filter((p) => p !== ""))].sort((a, b) => a.localeCompare(b, lang)),
    [snippets, lang],
  );
  const q = query.trim().toLocaleLowerCase();
  const shown = snippets.filter(
    (s) =>
      (filter === null || s.package === filter) &&
      (q === "" || s.name.toLocaleLowerCase().includes(q) || s.content.toLocaleLowerCase().includes(q)),
  );

  const packageButton = (value: Filter, label: string) => (
    <li key={value ?? "*all"}>
      <button className={filter === value ? "pane-entry active" : "pane-entry"} onClick={() => setFilter(value)}>
        {label}
      </button>
    </li>
  );

  return (
    <section className="page snippets">
      <header className="page-head">
        <h1>{t("nav.snippets")}</h1>
        <div className="page-actions">
          <button className="primary" onClick={() => setEditing({})}>
            {t("snippets.new")}
          </button>
        </div>
      </header>
      {problem !== null && <ErrorMessage error={problem} />}
      <div className="snippets-layout">
        <ul className="package-list" aria-label={t("snippets.packages")}>
          {packageButton(null, t("snippets.all"))}
          {packageButton("", t("snippets.noPackage"))}
          {packages.map((p) => packageButton(p, p))}
        </ul>
        <div className="snippets-main">
          <label className="search">
            <Search size={16} aria-hidden="true" />
            <input
              type="search"
              value={query}
              placeholder={t("snippets.search")}
              aria-label={t("snippets.search")}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          {shown.length === 0 && <p className="empty">{snippets.length === 0 ? t("snippets.empty") : t("snippets.noMatch")}</p>}
          <div className="list">
            {shown.map((s) => (
              <div
                key={s.id}
                className="list-row snippet-row"
                tabIndex={0}
                onDoubleClick={() => setEditing({ snippet: s })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({
                    at: { x: e.clientX, y: e.clientY },
                    items: [
                      { label: t("snippets.run"), onSelect: () => onRun(s) },
                      { label: t("snippets.edit"), onSelect: () => setEditing({ snippet: s }) },
                      { label: t("common.delete"), danger: true, onSelect: () => setConfirm(s) },
                    ],
                  });
                }}
              >
                <span className="palette-name">{s.name}</span>
                {s.package !== "" && <span className="card-sub">{s.package}</span>}
                <code className="card-sub snippet-first-line">{s.content.split(/\r?\n/)[0]}</code>
                <button className="icon" aria-label={t("snippets.run")} title={t("snippets.run")} onClick={() => onRun(s)}>
                  <Play size={14} />
                </button>
              </div>
            ))}
          </div>
        </div>
      </div>

      {menu && <ContextMenu items={menu.items} at={menu.at} onClose={() => setMenu(null)} />}

      {editing && (
        <Drawer onClose={() => setEditing(null)}>
          <SnippetForm
            snippet={editing.snippet}
            packages={packages}
            package={filter ?? ""}
            hosts={hosts}
            onDone={() => {
              setEditing(null);
              reload();
            }}
            onCancel={() => setEditing(null)}
          />
        </Drawer>
      )}

      {confirm && (
        <ConfirmDialog
          message={t("snippets.deleteConfirm", { name: confirm.name })}
          confirmLabel={t("common.delete")}
          onConfirm={() => {
            const gone = confirm;
            setConfirm(null);
            void snippetDelete(gone.id).then(reload, setProblem);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </section>
  );
}
