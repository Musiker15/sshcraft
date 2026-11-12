import { ChevronDown, Folder, LayoutGrid, List, Search, Server } from "lucide-react";
import { useMemo, useState } from "react";
import type { Group, Host, ItemKind, KeyInfo, ServerView } from "../api";
import { useLanguage, useT } from "../i18n/i18n";
import { useSettings } from "../settings/SettingsProvider";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";
import { filterHosts, hostSubtitle, sortByName } from "./hostsView";
import { missingKey } from "./sync/syncView";

export type HostsPageProps = {
  groups: Group[];
  hosts: Host[];
  onConnect: (host: Host) => void;
  onEditHost: (host: Host) => void;
  onDeleteHost: (host: Host) => void;
  onEditGroup: (group: Group) => void;
  onDeleteGroup: (group: Group) => void;
  onNewHost: () => void;
  onNewGroup: () => void;
  onImportSshConfig: () => void;
  /** Sync servers, for the location badge and "Move to". */
  servers?: ServerView[];
  /** Keys on this device, to point out hosts whose key is missing here. */
  keys?: KeyInfo[];
  onMove?: (kind: ItemKind, id: string, location: string | null) => void;
};

type Menu = { items: MenuItem[]; at: { x: number; y: number } };

/** Groups and hosts of the vault; double-click connects, right-click offers more. */
export function HostsPage(props: HostsPageProps) {
  const t = useT();
  const lang = useLanguage();
  const { settings, update } = useSettings();
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<Menu | null>(null);
  const view = settings.host_view;

  const groupsById = useMemo(() => new Map(props.groups.map((g) => [g.id, g])), [props.groups]);
  const groups = useMemo(() => sortByName(props.groups, lang), [props.groups, lang]);
  const hosts = useMemo(() => sortByName(props.hosts, lang), [props.hosts, lang]);
  const found = useMemo(() => filterHosts(hosts, props.groups, query), [hosts, props.groups, query]);
  const searching = query.trim() !== "";

  const servers = props.servers ?? [];
  const serverName = (id: string | null | undefined) => servers.find((s) => s.id === id)?.name;
  /** "Move to" entries: this device only and every server, except where the item is now. */
  const moveItems = (kind: ItemKind, id: string, location: string | null | undefined, followsGroup: boolean): MenuItem[] => {
    if (!props.onMove || servers.length === 0) return [];
    const places: { id: string | null; name: string }[] = [
      { id: null, name: t("sync.localOnly") },
      ...servers.map((s) => ({ id: s.id, name: s.name })),
    ];
    return places
      .filter((p) => p.id !== (location ?? null))
      .map((p) => ({
        label: t("sync.moveTo", { place: p.name }),
        onSelect: () => props.onMove?.(kind, id, p.id),
        disabled: followsGroup,
        title: followsGroup ? t("sync.followsGroup") : undefined,
      }));
  };

  const openMenu = (items: MenuItem[], e: React.MouseEvent) => {
    e.preventDefault();
    setMenu({ items, at: { x: e.clientX, y: e.clientY } });
  };

  const hostItem = (host: Host) => {
    const subtitle = missingKey(host, props.keys ?? [])
      ? t("hosts.keyMissing")
      : hostSubtitle(host, host.group_id ? groupsById.get(host.group_id) : undefined);
    const place = serverName(host.location);
    return (
      <div
        key={host.id}
        className={view === "grid" ? "card" : "card row"}
        tabIndex={0}
        onDoubleClick={() => props.onConnect(host)}
        onKeyDown={(e) => e.key === "Enter" && props.onConnect(host)}
        onContextMenu={(e) =>
          openMenu(
            [
              { label: t("hosts.connect"), onSelect: () => props.onConnect(host) },
              { label: t("hosts.edit"), onSelect: () => props.onEditHost(host) },
              ...moveItems("host", host.id, host.location, host.group_id !== null),
              { label: t("common.delete"), onSelect: () => props.onDeleteHost(host), danger: true },
            ],
            e,
          )
        }
      >
        <span className="card-icon">
          <Server size={18} aria-hidden="true" />
        </span>
        <span className="card-text">
          <span className="card-title">
            {host.name}
            {place && <span className="badge">{place}</span>}
          </span>
          <span className="card-sub">{subtitle ?? t("hosts.noAddress")}</span>
        </span>
      </div>
    );
  };

  const groupCard = (group: Group) => (
    <div
      key={group.id}
      className={view === "grid" ? "card" : "card row"}
      tabIndex={0}
      onDoubleClick={() => props.onEditGroup(group)}
      onContextMenu={(e) =>
        openMenu(
          [
            { label: t("hosts.edit"), onSelect: () => props.onEditGroup(group) },
            ...moveItems("group", group.id, group.location, false),
            { label: t("common.delete"), onSelect: () => props.onDeleteGroup(group), danger: true },
          ],
          e,
        )
      }
    >
      <span className="card-icon group">
        <Folder size={18} aria-hidden="true" />
      </span>
      <span className="card-text">
        <span className="card-title">
          {group.name}
          {serverName(group.location) && <span className="badge">{serverName(group.location)}</span>}
        </span>
        <span className="card-sub">
          {t("hosts.count", { count: props.hosts.filter((h) => h.group_id === group.id).length })}
        </span>
      </span>
    </div>
  );

  const section = (key: string, title: string, items: Host[]) =>
    items.length > 0 && (
      <div className="group-block" key={key}>
        <h2 className="group-title">{title}</h2>
        <div className={view === "grid" ? "cards" : "rows"}>{items.map(hostItem)}</div>
      </div>
    );

  return (
    <section className="page">
      <header className="page-head">
        <h1>{t("nav.hosts")}</h1>
        <div className="page-actions">
          <div className="segmented" role="group" aria-label={t("hosts.view")}>
            <button
              className={view === "grid" ? "icon active" : "icon"}
              aria-pressed={view === "grid"}
              aria-label={t("hosts.viewGrid")}
              title={t("hosts.viewGrid")}
              onClick={() => void update({ host_view: "grid" })}
            >
              <LayoutGrid size={16} />
            </button>
            <button
              className={view === "list" ? "icon active" : "icon"}
              aria-pressed={view === "list"}
              aria-label={t("hosts.viewList")}
              title={t("hosts.viewList")}
              onClick={() => void update({ host_view: "list" })}
            >
              <List size={16} />
            </button>
          </div>
          <NewMenu
            labels={{ host: t("hosts.newHost"), group: t("hosts.newGroup"), sshconfig: t("hosts.importSshConfig") }}
            button={t("hosts.new")}
            onHost={props.onNewHost}
            onGroup={props.onNewGroup}
            onSshConfig={props.onImportSshConfig}
          />
        </div>
      </header>
      <label className="search">
        <Search size={16} aria-hidden="true" />
        <input
          type="search"
          value={query}
          placeholder={t("hosts.search")}
          aria-label={t("hosts.search")}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>

      {props.hosts.length === 0 && props.groups.length === 0 && <p className="empty">{t("hosts.empty")}</p>}

      {searching ? (
        found.length === 0 ? (
          <p className="empty">{t("hosts.noMatch")}</p>
        ) : (
          <div className={view === "grid" ? "cards" : "rows"}>{found.map(hostItem)}</div>
        )
      ) : (
        <>
          {groups.length > 0 && (
            <div className="group-block">
              <h2 className="group-title">{t("hosts.groups")}</h2>
              <div className={view === "grid" ? "cards" : "rows"}>{groups.map(groupCard)}</div>
            </div>
          )}
          {section(
            "ungrouped",
            t("hosts.ungrouped"),
            hosts.filter((h) => !h.group_id),
          )}
          {groups.map((g) =>
            section(
              g.id,
              g.name,
              hosts.filter((h) => h.group_id === g.id),
            ),
          )}
        </>
      )}
      {menu && <ContextMenu items={menu.items} at={menu.at} onClose={() => setMenu(null)} />}
    </section>
  );
}

type NewMenuProps = {
  button: string;
  labels: { host: string; group: string; sshconfig: string };
  onHost: () => void;
  onGroup: () => void;
  onSshConfig: () => void;
};

/** "New" button with a small menu: host, group, import from ssh config. */
function NewMenu({ button, labels, onHost, onGroup, onSshConfig }: NewMenuProps) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <button
        className="primary"
        aria-haspopup="menu"
        onClick={(e) => {
          const box = e.currentTarget.getBoundingClientRect();
          setAt({ x: box.left, y: box.bottom + 4 });
        }}
      >
        {button} <ChevronDown size={14} aria-hidden="true" />
      </button>
      {at && (
        <ContextMenu
          at={at}
          onClose={() => setAt(null)}
          items={[
            { label: labels.host, onSelect: onHost },
            { label: labels.group, onSelect: onGroup },
            { label: labels.sshconfig, onSelect: onSshConfig },
          ]}
        />
      )}
    </>
  );
}
