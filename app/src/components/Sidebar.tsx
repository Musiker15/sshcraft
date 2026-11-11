import type { Group, Host } from "../api";

type Props = {
  groups: Group[];
  hosts: Host[];
  onConnect: (host: Host) => void;
  onEditHost: (host: Host) => void;
  onDeleteHost: (host: Host) => void;
  onEditGroup: (group: Group) => void;
  onDeleteGroup: (group: Group) => void;
  onNewHost: () => void;
  onNewGroup: () => void;
  onKeys: () => void;
};

export function Sidebar(props: Props) {
  const loose = props.hosts.filter((h) => !h.group_id);

  const hostRow = (host: Host, isLoose: boolean) => (
    <div
      key={host.id}
      className={isLoose ? "sidebar-row loose" : "sidebar-row"}
      onDoubleClick={() => props.onConnect(host)}
      title="Doppelklick zum Verbinden"
    >
      <span className="name">{host.name}</span>
      <span className="row-actions">
        <button className="link" onClick={() => props.onConnect(host)} title="Verbinden">
          ▶
        </button>
        <button className="link" onClick={() => props.onEditHost(host)} title="Bearbeiten">
          ✎
        </button>
        <button className="link" onClick={() => props.onDeleteHost(host)} title="Löschen">
          ✕
        </button>
      </span>
    </div>
  );

  return (
    <aside className="sidebar">
      <div className="sidebar-actions">
        <button onClick={props.onNewHost}>Neuer Host</button>
        <button onClick={props.onNewGroup}>Neue Gruppe</button>
        <button onClick={props.onKeys}>Schlüssel</button>
      </div>
      {props.groups.map((group) => (
        <details key={group.id} className="sidebar-group" open>
          <summary>
            <span className="name">{group.name}</span>
            <span className="row-actions">
              <button className="link" onClick={() => props.onEditGroup(group)} title="Bearbeiten">
                ✎
              </button>
              <button className="link" onClick={() => props.onDeleteGroup(group)} title="Löschen">
                ✕
              </button>
            </span>
          </summary>
          {props.hosts.filter((h) => h.group_id === group.id).map((h) => hostRow(h, false))}
        </details>
      ))}
      {loose.map((h) => hostRow(h, true))}
    </aside>
  );
}
