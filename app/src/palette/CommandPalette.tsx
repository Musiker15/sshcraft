import { Search, Server, Zap } from "lucide-react";
import { useMemo, useState } from "react";
import type { Group, Host } from "../api";
import { useT } from "../i18n/i18n";
import { hostSubtitle } from "../pages/hostsView";
import { rankPalette, type PaletteActionId } from "./rank";

const ACTIONS: PaletteActionId[] = [
  "newTab",
  "newHost",
  "newGroup",
  "importHosts",
  "exportHosts",
  "openHosts",
  "openKeys",
  "openSettings",
  "closeTab",
];

type Props = {
  hosts: Host[];
  groups: Group[];
  recent: string[];
  onClose: () => void;
  onHost: (host: Host) => void;
  onAction: (id: PaletteActionId) => void;
};

/** Search box over hosts and actions; arrows move, Enter picks, Escape closes. */
export function CommandPalette({ hosts, groups, recent, onClose, onHost, onAction }: Props) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const actions = useMemo(() => ACTIONS.map((id) => ({ id, label: t(`palette.action.${id}`) })), [t]);
  const items = useMemo(() => rankPalette(query, hosts, groups, recent, actions), [query, hosts, groups, recent, actions]);
  const current = Math.min(selected, Math.max(items.length - 1, 0));

  function pick(index: number) {
    const item = items[index];
    if (!item) return;
    onClose();
    if (item.kind === "host") onHost(item.host);
    else onAction(item.id);
  }

  return (
    <div className="overlay palette-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label={t("palette.title")}>
        <label className="search">
          <Search size={16} aria-hidden="true" />
          <input
            autoFocus
            value={query}
            placeholder={t("palette.placeholder")}
            aria-label={t("palette.placeholder")}
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            onChange={(e) => {
              setQuery(e.target.value);
              setSelected(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setSelected((s) => Math.min(s + 1, items.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSelected((s) => Math.max(s - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                pick(current);
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
          />
        </label>
        <ul id="palette-list" className="palette-list" role="listbox">
          {items.length === 0 && <li className="empty">{t("palette.none")}</li>}
          {items.map((item, index) => (
            <li
              key={item.kind === "host" ? `h:${item.host.id}` : `a:${item.id}`}
              role="option"
              aria-selected={index === current}
              className={index === current ? "palette-item active" : "palette-item"}
              onMouseEnter={() => setSelected(index)}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(index);
              }}
            >
              {item.kind === "host" ? (
                <>
                  <Server size={16} aria-hidden="true" />
                  <span className="palette-name">{item.host.name}</span>
                  <span className="card-sub">
                    {hostSubtitle(item.host, groups)}
                  </span>
                </>
              ) : (
                <>
                  <Zap size={16} aria-hidden="true" />
                  <span className="palette-name">{item.label}</span>
                </>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
