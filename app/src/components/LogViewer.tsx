// One session log, played into a read-only terminal, with its bookmarks and comments beside it,
// a search and the export.
import { save } from "@tauri-apps/plugin-dialog";
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { ArrowLeft, Bookmark, ChevronDown, ChevronUp, Download, MessageSquare, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { logExport, logRead, logSaveMarks, type LogExportFormat, type LogMark, type LogMeta, type MarkKind } from "../api";
import { useLanguage, useT } from "../i18n/i18n";
import { lineForOffset, offsetForLine, pieces, type LineAt } from "../sessionlog/marks";
import { useSettings } from "../settings/SettingsProvider";
import { terminalTheme } from "../settings/themes";
import { ErrorMessage } from "../shell/ErrorMessage";

type Props = {
  log: LogMeta;
  /** Searched for right after loading (from a hit on the logs page). */
  query?: string;
  onClose: () => void;
  onChanged: (meta: LogMeta) => void;
};

const MAX_BOOKMARK = 100;
const MAX_COMMENT = 2000;

/** Writes `bytes` in pieces and notes the cursor line after each. */
async function writeAll(term: Terminal, bytes: Uint8Array, cuts: number[]): Promise<LineAt[]> {
  const table: LineAt[] = [];
  let from = 0;
  for (const to of cuts) {
    await new Promise<void>((done) => term.write(bytes.subarray(from, to), done));
    const buffer = term.buffer.active;
    table.push({ offset: to, line: buffer.baseY + buffer.cursorY });
    from = to;
  }
  return table;
}

export function LogViewer({ log, query = "", onClose, onChanged }: Props) {
  const t = useT();
  const lang = useLanguage();
  const { settings } = useSettings();
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const search = useRef<SearchAddon | null>(null);
  const [table, setTable] = useState<LineAt[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [marks, setMarks] = useState<LogMark[]>(log.marks);
  const [find, setFind] = useState(query);
  const [draft, setDraft] = useState<{ kind: MarkKind; text: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [problem, setProblem] = useState<unknown>(null);

  useEffect(() => {
    if (!host.current) return;
    const terminal = new Terminal({
      disableStdin: true,
      cursorBlink: false,
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      theme: terminalTheme(settings.terminal_theme).colors,
      scrollback: 200_000,
    });
    const finder = new SearchAddon();
    terminal.loadAddon(finder);
    terminal.open(host.current);
    term.current = terminal;
    search.current = finder;
    let gone = false;
    logRead(log.id)
      .then(async (bytes) => {
        // Finer pieces place new marks closer; at most about 20000 of them keep loading quick.
        const step = Math.min(4096, Math.max(256, Math.ceil(bytes.length / 20000)));
        const cuts = pieces(
          bytes.length,
          log.marks.map((m) => m.offset),
          step,
        );
        const written = await writeAll(terminal, bytes, cuts);
        if (gone) return;
        setTable(written);
        setLoaded(true);
        if (query.trim() !== "") finder.findNext(query, { caseSensitive: false });
      })
      .catch((e: unknown) => {
        if (!gone) setProblem(e);
      });
    return () => {
      gone = true;
      terminal.dispose();
      term.current = null;
    };
    // The log is read once per log; settings changes do not replay it.
  }, [log.id]);

  const sorted = [...marks].sort((a, b) => a.offset - b.offset || a.created_at - b.created_at);

  function persist(next: LogMark[]) {
    logSaveMarks(log.id, next).then((meta) => {
      setMarks(meta.marks);
      onChanged(meta);
    }, setProblem);
  }

  function addMark() {
    if (!draft || !term.current) return;
    const text = draft.text.trim();
    if (draft.kind === "comment" && text === "") return;
    const offset = Math.min(offsetForLine(table, term.current.buffer.active.viewportY), log.size);
    const mark: LogMark = { id: crypto.randomUUID(), offset, kind: draft.kind, text, created_at: Date.now() };
    setDraft(null);
    persist([...marks, mark]);
  }

  function jump(mark: LogMark) {
    term.current?.scrollToLine(lineForOffset(table, mark.offset));
  }

  async function exportAs(format: LogExportFormat) {
    setExporting(false);
    const name = `${log.title}-${new Date(log.started_at).toISOString().slice(0, 10)}.${format === "text" ? "txt" : "log"}`;
    const path = await save({ defaultPath: name.replace(/[\\/:*?"<>|]/g, "_") });
    if (path) await logExport(log.id, path, format).catch(setProblem);
  }

  const findNext = (back = false) => {
    if (find.trim() === "") return;
    if (back) search.current?.findPrevious(find, { caseSensitive: false });
    else search.current?.findNext(find, { caseSensitive: false });
  };

  return (
    <section className="page log-viewer">
      <header className="page-head">
        <button className="icon" aria-label={t("logs.back")} title={t("logs.back")} onClick={onClose}>
          <ArrowLeft size={16} />
        </button>
        <h1>{log.title}</h1>
        <span className="card-sub">{new Date(log.started_at).toLocaleString(lang)}</span>
        <div className="page-actions">
          <input
            type="search"
            value={find}
            placeholder={t("logs.find")}
            aria-label={t("logs.find")}
            onChange={(e) => setFind(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") findNext(e.shiftKey);
            }}
          />
          <button className="icon" aria-label={t("logs.findPrev")} title={t("logs.findPrev")} onClick={() => findNext(true)}>
            <ChevronUp size={16} />
          </button>
          <button className="icon" aria-label={t("logs.findNext")} title={t("logs.findNext")} onClick={() => findNext()}>
            <ChevronDown size={16} />
          </button>
          <button disabled={!loaded} onClick={() => setDraft({ kind: "bookmark", text: "" })}>
            <Bookmark size={14} /> {t("logs.addBookmark")}
          </button>
          <button disabled={!loaded} onClick={() => setDraft({ kind: "comment", text: "" })}>
            <MessageSquare size={14} /> {t("logs.addComment")}
          </button>
          <div className="menu-anchor">
            <button onClick={() => setExporting(!exporting)} aria-expanded={exporting}>
              <Download size={14} /> {t("logs.export")}
            </button>
            {exporting && (
              <div className="menu" role="menu">
                <button role="menuitem" onClick={() => void exportAs("text")}>
                  {t("logs.exportText")}
                </button>
                <button role="menuitem" onClick={() => void exportAs("raw")}>
                  {t("logs.exportRaw")}
                </button>
              </div>
            )}
          </div>
        </div>
      </header>
      {problem !== null && <ErrorMessage error={problem} />}
      {log.truncated && <p className="notice">{t("logs.truncated")}</p>}
      <div className="log-viewer-layout">
        <div className="log-terminal" ref={host} data-testid="log-terminal" />
        <aside className="log-marks" aria-label={t("logs.marks")}>
          {draft && (
            <form
              className="mark-form"
              onSubmit={(e) => {
                e.preventDefault();
                addMark();
              }}
            >
              {draft.kind === "comment" ? (
                <textarea
                  autoFocus
                  aria-label={t("logs.comment")}
                  maxLength={MAX_COMMENT}
                  value={draft.text}
                  onChange={(e) => setDraft({ ...draft, text: e.target.value })}
                />
              ) : (
                <input
                  autoFocus
                  aria-label={t("logs.bookmarkName")}
                  maxLength={MAX_BOOKMARK}
                  value={draft.text}
                  onChange={(e) => setDraft({ ...draft, text: e.target.value.replace(/[\r\n]/g, " ") })}
                />
              )}
              <p className="card-sub">{t("logs.markHere")}</p>
              <div className="form-actions">
                <button type="button" onClick={() => setDraft(null)}>
                  {t("common.cancel")}
                </button>
                <button className="primary" type="submit">
                  {t("common.save")}
                </button>
              </div>
            </form>
          )}
          {sorted.length === 0 && !draft && <p className="empty">{t("logs.noMarks")}</p>}
          <ul>
            {sorted.map((m) => (
              <li key={m.id} className={`mark mark-${m.kind}`}>
                <button className="link mark-jump" onClick={() => jump(m)}>
                  {m.kind === "bookmark" ? <Bookmark size={12} /> : <MessageSquare size={12} />}
                  <span>{m.text || t("logs.bookmark")}</span>
                </button>
                <span className="card-sub">{new Date(m.created_at).toLocaleString(lang)}</span>
                <button
                  className="icon"
                  aria-label={t("logs.removeMark")}
                  title={t("logs.removeMark")}
                  onClick={() => persist(marks.filter((x) => x.id !== m.id))}
                >
                  <Trash2 size={12} />
                </button>
              </li>
            ))}
          </ul>
        </aside>
      </div>
    </section>
  );
}
