// Session logs: every recording with its host, time, size and place, a search through all of
// them, and deleting one, some or all. Opening one shows it in the viewer.
import { Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { listServers, logsDelete, logsList, logsSearch, type Host, type LogHit, type LogSummary } from "../api";
import { LogViewer } from "../components/LogViewer";
import { useLanguage, useT } from "../i18n/i18n";
import { ConfirmDialog } from "../shell/ConfirmDialog";
import { ErrorMessage } from "../shell/ErrorMessage";

type Props = { hosts: Host[] };
/** `null`: every log; `""`: logs without a host (local, serial); else a host id. */
type HostFilter = string | null;

const MIN_QUERY = 2;

export function formatSize(bytes: number, lang: string): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toLocaleString(lang, { maximumFractionDigits: unit === 0 ? 0 : 1 })} ${units[unit]}`;
}

export function LogsPage({ hosts }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [logs, setLogs] = useState<LogSummary[]>([]);
  const [servers, setServers] = useState<Record<string, string>>({});
  const [filter, setFilter] = useState<HostFilter>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<LogHit[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const [open, setOpen] = useState<{ log: LogSummary; query: string } | null>(null);
  const [problem, setProblem] = useState<unknown>(null);

  const load = useCallback(() => {
    logsList().then((list) => setLogs(list ?? []), setProblem);
  }, []);
  useEffect(load, [load]);
  useEffect(() => {
    listServers().then(
      (list) => setServers(Object.fromEntries((list ?? []).map((s) => [s.id, s.name]))),
      () => setServers({}),
    );
  }, []);

  // Searching all logs opens each one, so it waits until typing pauses.
  const q = query.trim();
  useEffect(() => {
    if (q.length < MIN_QUERY) return;
    let current = true;
    const timer = window.setTimeout(() => {
      logsSearch(q).then((found) => {
        if (current) setHits(found ?? []);
      }, setProblem);
    }, 300);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [q]);
  const shownHits = q.length >= MIN_QUERY ? hits : null;

  const hostName = useMemo(() => new Map(hosts.map((h) => [h.id, h.name])), [hosts]);
  const byId = useMemo(() => new Map(logs.map((l) => [l.id, l])), [logs]);
  const shown = logs.filter((l) => filter === null || (filter === "" ? l.host_id === null : l.host_id === filter));
  const logHosts = [...new Set(logs.map((l) => l.host_id).filter((h): h is string => h !== null))];

  const toggle = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  if (open) {
    return (
      <LogViewer
        log={open.log}
        query={open.query}
        onClose={() => {
          setOpen(null);
          load();
        }}
        onChanged={load}
      />
    );
  }

  const where = (l: LogSummary) => {
    if (l.kind === "local") return t("logs.kindLocal");
    if (l.kind === "serial") return t("logs.kindSerial");
    return (l.host_id && hostName.get(l.host_id)) || t("logs.hostGone");
  };

  return (
    <section className="page logs">
      <header className="page-head">
        <h1>{t("nav.logs")}</h1>
        <div className="page-actions">
          <button className="danger" disabled={selected.size === 0} onClick={() => setConfirm([...selected])}>
            {t("logs.deleteSelected", { count: selected.size })}
          </button>
          <button className="danger" disabled={shown.length === 0} onClick={() => setConfirm(shown.map((l) => l.id))}>
            {t("logs.deleteShown")}
          </button>
        </div>
      </header>
      {problem !== null && <ErrorMessage error={problem} />}
      <div className="logs-tools">
        <label className="search">
          <Search size={16} aria-hidden="true" />
          <input
            type="search"
            value={query}
            placeholder={t("logs.search")}
            aria-label={t("logs.search")}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <select
          aria-label={t("logs.filterHost")}
          value={filter ?? "*"}
          onChange={(e) => setFilter(e.target.value === "*" ? null : e.target.value)}
        >
          <option value="*">{t("logs.allHosts")}</option>
          <option value="">{t("logs.withoutHost")}</option>
          {logHosts.map((id) => (
            <option key={id} value={id}>
              {hostName.get(id) ?? t("logs.hostGone")}
            </option>
          ))}
        </select>
      </div>

      {shownHits !== null ? (
        <div className="list" aria-label={t("logs.hits")}>
          {shownHits.length === 0 && <p className="empty">{t("logs.noHits")}</p>}
          {shownHits.map((h, i) => {
            const log = byId.get(h.log_id);
            if (!log) return null;
            return (
              <button
                key={`${h.log_id}-${h.line}-${i}`}
                className="list-row hit-row"
                onClick={() => setOpen({ log, query: q })}
              >
                <span className="palette-name">{log.title}</span>
                <span className="card-sub">{new Date(log.started_at).toLocaleString(lang)}</span>
                <code className="card-sub">{h.snippet}</code>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="list">
          {shown.length === 0 && <p className="empty">{logs.length === 0 ? t("logs.empty") : t("logs.noMatch")}</p>}
          {shown.map((l) => (
            <div key={l.id} className="list-row log-row" onDoubleClick={() => setOpen({ log: l, query: "" })}>
              <input
                type="checkbox"
                aria-label={t("logs.select", { name: l.title })}
                checked={selected.has(l.id)}
                onChange={() => toggle(l.id)}
              />
              <button className="link palette-name" onClick={() => setOpen({ log: l, query: "" })}>
                {l.title}
              </button>
              <span className="card-sub">{where(l)}</span>
              <span className="card-sub">{new Date(l.started_at).toLocaleString(lang)}</span>
              <span className="card-sub">
                {l.ended_at === null
                  ? t("logs.running")
                  : t("logs.minutes", { count: Math.max(1, Math.round((l.ended_at - l.started_at) / 60000)) })}
              </span>
              <span className="card-sub">{formatSize(l.size, lang)}</span>
              <span className="badge">
                {l.location ? (servers[l.location] ?? t("logs.server")) : t("logs.thisDevice")}
              </span>
            </div>
          ))}
        </div>
      )}

      {confirm && (
        <ConfirmDialog
          message={t("logs.deleteConfirm", { count: confirm.length })}
          confirmLabel={t("common.delete")}
          onConfirm={() => {
            const ids = confirm;
            setConfirm(null);
            setSelected(new Set());
            void logsDelete(ids).then(load, setProblem);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </section>
  );
}
