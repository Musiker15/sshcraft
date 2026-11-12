import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { listServers, removeServer, setServerSync, type AccountCreated, type ServerView } from "../../api";
import { useT } from "../../i18n/i18n";
import { ConfirmDialog } from "../../shell/ConfirmDialog";
import { Drawer } from "../../shell/Drawer";
import { ErrorMessage } from "../../shell/ErrorMessage";
import { AccountKeyDialog } from "./AccountKeyDialog";
import { AddDeviceDialog } from "./AddDeviceDialog";
import { AddServerForm } from "./AddServerForm";
import { DevicesPanel } from "./DevicesPanel";
import { statusTone } from "./syncView";

type Panel =
  | { kind: "add" }
  | { kind: "created"; created: AccountCreated }
  | { kind: "devices"; server: ServerView }
  | { kind: "addDevice" };

/** Sync servers of this device, in the settings. */
export function SyncSection({ onChanged }: { onChanged: () => void }) {
  const t = useT();
  const [servers, setServers] = useState<ServerView[]>([]);
  const [panel, setPanel] = useState<Panel | null>(null);
  const [removing, setRemoving] = useState<ServerView | null>(null);
  const [warned, setWarned] = useState<Set<string>>(new Set());
  const [error, setError] = useState<unknown>(null);

  const load = useCallback(() => {
    listServers().then((list) => setServers(list ?? []), setError);
  }, []);

  useEffect(() => {
    load();
    const listeners = [
      listen("sync-status", () => load()),
      listen<{ server_id: string }>("sync-warning", (event) =>
        setWarned((set) => new Set(set).add(event.payload.server_id)),
      ),
    ];
    return () => {
      for (const unlisten of listeners) void unlisten.then((stop) => stop());
    };
  }, [load]);

  async function toggle(server: ServerView, syncEnabled: boolean, syncKeys: boolean) {
    try {
      await setServerSync(server.id, syncEnabled, syncKeys);
      load();
      onChanged();
    } catch (e) {
      setError(e);
    }
  }

  async function remove(server: ServerView) {
    setRemoving(null);
    try {
      await removeServer(server.id);
      load();
      onChanged();
    } catch (e) {
      setError(e);
    }
  }

  const statusText = (server: ServerView) => {
    const s = server.status;
    return s.state === "offline" ? t("sync.status.offline", { seconds: s.retry_in_secs }) : t(`sync.status.${statusTone(s)}`);
  };

  const closePanel = () => {
    setPanel(null);
    load();
    onChanged();
  };

  return (
    <div className="settings-block">
      <h2 className="group-title">{t("sync.title")}</h2>
      {servers.length === 0 && <p className="hint">{t("sync.empty")}</p>}
      {error !== null && <ErrorMessage error={error} />}
      <div className="rows">
        {servers.map((server) => (
          <div key={server.id} className="card row sync-server">
            <span className={`status-dot ${statusTone(server.status)}`} aria-hidden="true" />
            <span className="card-text">
              <span className="card-title">{server.name}</span>
              <span className="card-sub">
                {server.origin} · {statusText(server)}
              </span>
              {warned.has(server.id) && <span className="card-sub warning">{t("sync.warning")}</span>}
            </span>
            <label className="check">
              <input
                type="checkbox"
                checked={server.sync_enabled}
                disabled={server.revoked}
                onChange={(e) => void toggle(server, e.target.checked, server.sync_keys)}
              />
              {t("sync.syncOn")}
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={server.sync_keys}
                disabled={server.revoked}
                onChange={(e) => void toggle(server, server.sync_enabled, e.target.checked)}
              />
              {t("sync.syncKeys")}
            </label>
            <button disabled={server.revoked} onClick={() => setPanel({ kind: "devices", server })}>
              {t("sync.devices")}
            </button>
            <button className="danger" onClick={() => setRemoving(server)}>
              {t("sync.remove")}
            </button>
          </div>
        ))}
      </div>
      <div className="buttons">
        <button onClick={() => setPanel({ kind: "addDevice" })} disabled={servers.length === 0}>
          {t("sync.addDevice")}
        </button>
        <button className="primary" onClick={() => setPanel({ kind: "add" })}>
          {t("sync.add")}
        </button>
      </div>

      {panel && (
        <Drawer onClose={panel.kind === "created" ? () => {} : closePanel}>
          {panel.kind === "add" && (
            <AddServerForm
              onCreated={(created) => setPanel({ kind: "created", created })}
              onJoined={closePanel}
              onCancel={() => setPanel(null)}
            />
          )}
          {panel.kind === "created" && <AccountKeyDialog created={panel.created} onClose={closePanel} />}
          {panel.kind === "devices" && <DevicesPanel server={panel.server} onClose={closePanel} />}
          {panel.kind === "addDevice" && <AddDeviceDialog onClose={() => setPanel(null)} />}
        </Drawer>
      )}
      {removing && (
        <ConfirmDialog
          message={t("sync.removeConfirm", { name: removing.name })}
          confirmLabel={t("sync.remove")}
          onConfirm={() => void remove(removing)}
          onCancel={() => setRemoving(null)}
        />
      )}
    </div>
  );
}
