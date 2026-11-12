import { Folder, Server } from "lucide-react";
import { useState } from "react";
import type { Group, Host } from "../api";
import { useT } from "../i18n/i18n";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";

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
};

type Menu = { items: MenuItem[]; at: { x: number; y: number } };

/** Groups and hosts of the vault; double-click connects, right-click offers more. */
export function HostsPage(props: HostsPageProps) {
  const t = useT();
  const [menu, setMenu] = useState<Menu | null>(null);

  const hostMenu = (host: Host, at: Menu["at"]) =>
    setMenu({
      at,
      items: [
        { label: t("hosts.connect"), onSelect: () => props.onConnect(host) },
        { label: t("hosts.edit"), onSelect: () => props.onEditHost(host) },
        { label: t("common.delete"), onSelect: () => props.onDeleteHost(host), danger: true },
      ],
    });
  const groupMenu = (group: Group, at: Menu["at"]) =>
    setMenu({
      at,
      items: [
        { label: t("hosts.edit"), onSelect: () => props.onEditGroup(group) },
        { label: t("common.delete"), onSelect: () => props.onDeleteGroup(group), danger: true },
      ],
    });

  const hostCard = (host: Host) => (
    <div
      key={host.id}
      className="card"
      tabIndex={0}
      onDoubleClick={() => props.onConnect(host)}
      onKeyDown={(e) => e.key === "Enter" && props.onConnect(host)}
      onContextMenu={(e) => {
        e.preventDefault();
        hostMenu(host, { x: e.clientX, y: e.clientY });
      }}
    >
      <Server size={20} aria-hidden="true" />
      <span className="card-title">{host.name}</span>
    </div>
  );

  return (
    <section className="page">
      <header className="page-head">
        <h1>{t("nav.hosts")}</h1>
        <div className="page-actions">
          <button onClick={props.onImportSshConfig}>{t("hosts.importSshConfig")}</button>
          <button onClick={props.onNewGroup}>{t("hosts.newGroup")}</button>
          <button className="primary" onClick={props.onNewHost}>
            {t("hosts.newHost")}
          </button>
        </div>
      </header>
      {props.hosts.length === 0 && props.groups.length === 0 && <p className="empty">{t("hosts.empty")}</p>}
      {props.groups.map((group) => (
        <div key={group.id} className="group-block">
          <h2
            className="group-title"
            onContextMenu={(e) => {
              e.preventDefault();
              groupMenu(group, { x: e.clientX, y: e.clientY });
            }}
          >
            <Folder size={16} aria-hidden="true" />
            {group.name}
          </h2>
          <div className="cards">{props.hosts.filter((h) => h.group_id === group.id).map(hostCard)}</div>
        </div>
      ))}
      {props.hosts.some((h) => !h.group_id) && (
        <div className="group-block">
          <h2 className="group-title">{t("hosts.ungrouped")}</h2>
          <div className="cards">{props.hosts.filter((h) => !h.group_id).map(hostCard)}</div>
        </div>
      )}
      {menu && <ContextMenu items={menu.items} at={menu.at} onClose={() => setMenu(null)} />}
    </section>
  );
}
