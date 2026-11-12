import { Server } from "lucide-react";
import type { Host } from "../api";
import { useT } from "../i18n/i18n";

/** A fresh tab: pick the host it should connect to. */
export function NewTabPage({ hosts, onPick }: { hosts: Host[]; onPick: (host: Host) => void }) {
  const t = useT();
  return (
    <section className="page newtab">
      <h1>{t("newTab.title")}</h1>
      {hosts.length === 0 && <p className="empty">{t("hosts.empty")}</p>}
      <div className="list">
        {hosts.map((host) => (
          <button key={host.id} className="list-row" onClick={() => onPick(host)}>
            <Server size={16} aria-hidden="true" />
            <span>{host.name}</span>
          </button>
        ))}
      </div>
    </section>
  );
}
