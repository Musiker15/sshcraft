import { Cable, Clock, LayoutGrid, Search, Server, SquareTerminal, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import type { Group, Host, SerialSettings, ShellCommand, WorkspaceTemplate } from "../api";
import { SerialForm } from "../components/SerialForm";
import { useLanguage, useT } from "../i18n/i18n";
import { rankPalette } from "../palette/rank";
import { hostSubtitle, sortByName } from "./hostsView";

type Props = {
  hosts: Host[];
  groups: Group[];
  recent: string[];
  onPick: (host: Host) => void;
  /** Local shells (empty on a phone, where both local sections are left out). */
  shells?: ShellCommand[];
  onShell?: (shell: ShellCommand) => void;
  onSerial?: (settings: SerialSettings) => void;
  /** Workspace templates; left out where a workspace cannot open (inside a pane). */
  templates?: WorkspaceTemplate[];
  onTemplate?: (template: WorkspaceTemplate) => void;
  onDeleteTemplate?: (template: WorkspaceTemplate) => void;
};

const RECENT = 10;

/** A fresh tab: search, recent connections and all hosts; picking one connects this tab. */
export function NewTabPage({
  hosts,
  groups,
  recent,
  onPick,
  shells = [],
  onShell,
  onSerial,
  templates = [],
  onTemplate,
  onDeleteTemplate,
}: Props) {
  const t = useT();
  const lang = useLanguage();
  const [query, setQuery] = useState("");
  const matches = useMemo(
    () => rankPalette(query, hosts, groups, recent, []).flatMap((item) => (item.kind === "host" ? [item.host] : [])),
    [query, hosts, groups, recent],
  );
  const recentHosts = useMemo(() => {
    const byId = new Map(hosts.map((h) => [h.id, h]));
    return recent.flatMap((id) => byId.get(id) ?? []).slice(0, RECENT);
  }, [hosts, recent]);
  const sections = useMemo(() => {
    const loose = sortByName(hosts.filter((h) => !h.group_id), lang);
    const grouped = sortByName(groups, lang).map((g) => ({
      key: g.id,
      title: g.name,
      hosts: sortByName(hosts.filter((h) => h.group_id === g.id), lang),
    }));
    return [{ key: "ungrouped", title: t("hosts.ungrouped"), hosts: loose }, ...grouped].filter(
      (s) => s.hosts.length > 0,
    );
  }, [hosts, groups, lang, t]);

  const row = (host: Host, icon: "recent" | "host" = "host") => (
    <button key={`${icon}:${host.id}`} className="list-row" onClick={() => onPick(host)}>
      {icon === "recent" ? <Clock size={16} aria-hidden="true" /> : <Server size={16} aria-hidden="true" />}
      <span className="palette-name">{host.name}</span>
      <span className="card-sub">{hostSubtitle(host, groups)}</span>
    </button>
  );

  return (
    <section className="page newtab">
      <h1>{t("newTab.title")}</h1>
      <label className="search">
        <Search size={16} aria-hidden="true" />
        <input
          autoFocus
          type="search"
          value={query}
          placeholder={t("hosts.search")}
          aria-label={t("hosts.search")}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && matches[0]) onPick(matches[0]);
          }}
        />
      </label>
      {hosts.length === 0 && <p className="empty">{t("hosts.empty")}</p>}
      {query.trim() ? (
        <div className="list">
          {matches.length === 0 && <p className="empty">{t("hosts.noMatch")}</p>}
          {matches.map((host) => row(host))}
        </div>
      ) : (
        <>
          {templates.length > 0 && onTemplate && (
            <div className="group-block">
              <h2 className="group-title">{t("workspaces.title")}</h2>
              <div className="list">
                {templates.map((template) => (
                  <div key={template.id} className="list-row-wrap">
                    <button className="list-row" onClick={() => onTemplate(template)}>
                      <LayoutGrid size={16} aria-hidden="true" />
                      <span className="palette-name">{template.name}</span>
                      <span className="card-sub">{t("workspaces.panes", { count: template.panes.length })}</span>
                    </button>
                    {onDeleteTemplate && (
                      <button
                        className="icon"
                        aria-label={t("workspaces.delete", { name: template.name })}
                        title={t("workspaces.delete", { name: template.name })}
                        onClick={() => onDeleteTemplate(template)}
                      >
                        <Trash2 size={14} />
                      </button>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
          {recentHosts.length > 0 && (
            <div className="group-block">
              <h2 className="group-title">{t("newTab.recent")}</h2>
              <div className="list">{recentHosts.map((host) => row(host, "recent"))}</div>
            </div>
          )}
          {sections.map((s) => (
            <div key={s.key} className="group-block">
              <h2 className="group-title">{s.title}</h2>
              <div className="list">{s.hosts.map((host) => row(host))}</div>
            </div>
          ))}
          {shells.length > 0 && onShell && (
            <div className="group-block">
              <h2 className="group-title">{t("newTab.local")}</h2>
              <div className="list">
                {shells.map((shell) => (
                  <button key={`${shell.program} ${shell.args.join(" ")}`} className="list-row" onClick={() => onShell(shell)}>
                    <SquareTerminal size={16} aria-hidden="true" />
                    <span className="palette-name">{shell.label}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {shells.length > 0 && onSerial && (
            <div className="group-block">
              <h2 className="group-title">
                <Cable size={14} aria-hidden="true" /> {t("newTab.serial")}
              </h2>
              <SerialForm onOpen={onSerial} />
            </div>
          )}
        </>
      )}
    </section>
  );
}
