import { ChevronDown, ChevronRight, Folder, LayoutGrid, List, Search } from "lucide-react";
import { useMemo, useState } from "react";
import type { Group, Host, ItemKind, ItemShare, KeyInfo, ServerView } from "../api";
import { useLanguage, useT } from "../i18n/i18n";
import { useSettings } from "../settings/SettingsProvider";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";
import { HostIcon } from "../components/HostIcon";
import { allTags, chain, childGroups, hasTags, hostTags } from "./hostsTree";
import { filterHosts, hostSubtitle, sortByName } from "./hostsView";
import type { ShareTarget } from "./sync/ShareDialog";
import { missingKey } from "./sync/syncView";

export type HostsPageProps = {
  groups: Group[];
  hosts: Host[];
  onConnect: (host: Host) => void;
  onEditHost: (host: Host) => void;
  onDeleteHost: (host: Host) => void;
  onEditGroup: (group: Group) => void;
  onDeleteGroup: (group: Group) => void;
  /** `group`: the group shown at the moment, for the new item to go into. */
  onNewHost: (group?: string | null) => void;
  onNewGroup: (parent?: string | null) => void;
  onImportSshConfig: () => void;
  onExport?: () => void;
  /** Sync servers, for the location badge and "Move to". */
  servers?: ServerView[];
  /** Keys on this device, to point out hosts whose key is missing here. */
  keys?: KeyInfo[];
  onMove?: (kind: ItemKind, id: string, location: string | null) => void;
  /** Which items are shared or came in a share. */
  shares?: ItemShare[];
  onShare?: (target: ShareTarget, server: ServerView) => void;
  /** Opens "add a key to authorized_keys" for this host. */
  onInstallKey?: (host: Host) => void;
};

type Menu = { items: MenuItem[]; at: { x: number; y: number } };

/** Groups and hosts of the vault; double-click connects, right-click offers more. */
export function HostsPage(props: HostsPageProps) {
  const t = useT();
  const lang = useLanguage();
  const { settings, update } = useSettings();
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<Menu | null>(null);
  const [level, setLevel] = useState<string | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const view = settings.host_view;

  // A level whose group is gone (deleted, moved away) falls back to the top.
  const here = level && props.groups.some((g) => g.id === level) ? level : null;
  const groups = useMemo(() => sortByName(childGroups(props.groups, here), lang), [props.groups, here, lang]);
  const hosts = useMemo(() => sortByName(props.hosts, lang), [props.hosts, lang]);
  const tagChoices = useMemo(() => allTags([...props.hosts, ...props.groups]), [props.hosts, props.groups]);
  const found = useMemo(
    () => filterHosts(hosts, props.groups, query).filter((h) => hasTags({ tags: hostTags(h, props.groups) }, tags)),
    [hosts, props.groups, query, tags],
  );
  const searching = query.trim() !== "" || tags.length > 0;
  const trail = chain(props.groups, here).reverse();

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

  const shareOf = (kind: ItemKind, id: string) => props.shares?.find((s) => s.kind === kind && s.id === id);
  const shareBadge = (share: ItemShare | undefined) =>
    share && (
      <span className="badge share">
        {share.owned
          ? t("share.sharedBadge")
          : share.owner_name
            ? t("share.fromBadge", { name: share.owner_name })
            : t("share.fromSomeone")}
      </span>
    );
  /** "Share" for an item at a server that this account owns. */
  const shareItems = (target: ShareTarget, location: string | null | undefined, share: ItemShare | undefined): MenuItem[] => {
    const server = servers.find((s) => s.id === location);
    if (!props.onShare || !server || server.revoked || (share && !share.owned)) return [];
    return [{ label: t("share.menu"), onSelect: () => props.onShare?.(target, server) }];
  };

  const openMenu = (items: MenuItem[], e: React.MouseEvent) => {
    e.preventDefault();
    setMenu({ items, at: { x: e.clientX, y: e.clientY } });
  };

  const hostItem = (host: Host) => {
    const subtitle = missingKey(host, props.keys ?? [])
      ? t("hosts.keyMissing")
      : hostSubtitle(host, props.groups);
    const place = serverName(host.location);
    const share = shareOf("host", host.id);
    const received = share !== undefined && !share.owned;
    const readOnly = received && share.role !== "write";
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
              // A read-only host can still be edited to assign an own key.
              { label: t("hosts.edit"), onSelect: () => props.onEditHost(host) },
              ...(props.onInstallKey
                ? [{ label: t("hosts.installKey"), onSelect: () => props.onInstallKey?.(host) }]
                : []),
              ...shareItems({ kind: "host", host }, host.location, share),
              ...(received ? [] : moveItems("host", host.id, host.location, host.group_id !== null)),
              ...(readOnly ? [] : [{ label: t("common.delete"), onSelect: () => props.onDeleteHost(host), danger: true }]),
            ],
            e,
          )
        }
      >
        <span className="card-icon">
          <HostIcon os={host.os} />
        </span>
        <span className="card-text">
          <span className="card-title">
            {host.name}
            {place && <span className="badge">{place}</span>}
            {shareBadge(share)}
          </span>
          <span className="card-sub">{subtitle ?? t("hosts.noAddress")}</span>
          {(host.tags ?? []).length > 0 && (
            <span className="card-tags">
              {(host.tags ?? []).map((tag) => (
                <span key={tag} className="chip small">
                  {tag}
                </span>
              ))}
            </span>
          )}
        </span>
      </div>
    );
  };

  const groupCard = (group: Group) => {
    const share = shareOf("group", group.id);
    const received = share !== undefined && !share.owned;
    return (
    <div
      key={group.id}
      className={view === "grid" ? "card" : "card row"}
      tabIndex={0}
      onDoubleClick={() => setLevel(group.id)}
      onKeyDown={(e) => e.key === "Enter" && setLevel(group.id)}
      onContextMenu={(e) =>
        openMenu(
          [
            { label: t("hosts.openGroup"), onSelect: () => setLevel(group.id) },
            { label: t("hosts.edit"), onSelect: () => props.onEditGroup(group) },
            ...(received ? [] : [{ label: t("hosts.newSubgroup"), onSelect: () => props.onNewGroup(group.id) }]),
            ...shareItems({ kind: "group", group }, group.location, share),
            ...(received ? [] : moveItems("group", group.id, group.location, (group.parent_id ?? null) !== null)),
            ...(received ? [] : [{ label: t("common.delete"), onSelect: () => props.onDeleteGroup(group), danger: true }]),
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
          {shareBadge(share)}
        </span>
        <span className="card-sub">
          {[
            t("hosts.count", { count: props.hosts.filter((h) => h.group_id === group.id).length }),
            ...(childGroups(props.groups, group.id).length > 0
              ? [t("hosts.subgroups", { count: childGroups(props.groups, group.id).length })]
              : []),
          ].join(" · ")}
        </span>
      </span>
    </div>
    );
  };

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
            labels={{
              host: t("hosts.newHost"),
              group: t("hosts.newGroup"),
              sshconfig: t("hosts.import"),
              export: t("hosts.export"),
            }}
            button={t("hosts.new")}
            onHost={() => props.onNewHost(here)}
            onGroup={() => props.onNewGroup(here)}
            onSshConfig={props.onImportSshConfig}
            onExport={props.onExport}
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

      {tagChoices.length > 0 && (
        <div className="tag-filter" role="group" aria-label={t("hosts.tagFilter")}>
          {tagChoices.map((tag) => {
            const on = tags.some((x) => x.toLocaleLowerCase() === tag.toLocaleLowerCase());
            return (
              <button
                key={tag}
                className={on ? "chip active" : "chip"}
                aria-pressed={on}
                onClick={() =>
                  setTags(
                    on ? tags.filter((x) => x.toLocaleLowerCase() !== tag.toLocaleLowerCase()) : [...tags, tag],
                  )
                }
              >
                {tag}
              </button>
            );
          })}
        </div>
      )}

      {here && !searching && (
        <nav className="breadcrumbs" aria-label={t("hosts.path")}>
          <button className="link" onClick={() => setLevel(null)}>
            {t("hosts.allGroups")}
          </button>
          {trail.map((g) => (
            <span key={g.id}>
              <ChevronRight size={14} aria-hidden="true" />
              <button className="link" onClick={() => setLevel(g.id)} aria-current={g.id === here ? "page" : undefined}>
                {g.name}
              </button>
            </span>
          ))}
        </nav>
      )}

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
            "here",
            here ? t("hosts.inThisGroup") : t("hosts.ungrouped"),
            hosts.filter((h) => (h.group_id ?? null) === here),
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
  labels: { host: string; group: string; sshconfig: string; export: string };
  onHost: () => void;
  onGroup: () => void;
  onSshConfig: () => void;
  onExport?: () => void;
};

/** "New" button with a small menu: host, group, import, export. */
function NewMenu({ button, labels, onHost, onGroup, onSshConfig, onExport }: NewMenuProps) {
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
            ...(onExport ? [{ label: labels.export, onSelect: onExport }] : []),
          ]}
        />
      )}
    </>
  );
}
