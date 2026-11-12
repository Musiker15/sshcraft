import { useCallback, useEffect, useState } from "react";
import { listSyncDevices, revokeSyncDevice, type ServerView, type SyncDevice } from "../../api";
import { useLanguage, useT } from "../../i18n/i18n";
import { ConfirmDialog } from "../../shell/ConfirmDialog";
import { ErrorMessage } from "../../shell/ErrorMessage";

/** The account's devices on one server; any other device can be revoked from here. */
export function DevicesPanel({ server, onClose }: { server: ServerView; onClose: () => void }) {
  const t = useT();
  const lang = useLanguage();
  const [devices, setDevices] = useState<SyncDevice[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [revoking, setRevoking] = useState<SyncDevice | null>(null);

  const load = useCallback(() => {
    listSyncDevices(server.id).then(
      (list) => {
        setDevices(list);
        setError(null);
      },
      setError,
    );
  }, [server.id]);
  useEffect(load, [load]);

  async function revoke(device: SyncDevice) {
    setRevoking(null);
    try {
      await revokeSyncDevice(server.id, device.device_id);
      load();
    } catch (e) {
      setError(e);
    }
  }

  const seen = (at: number | null) =>
    at === null ? null : t("sync.lastSeen", { date: new Date(at * 1000).toLocaleString(lang) });

  return (
    <div className="dialog">
      <h2>
        {t("sync.devices")}: {server.name}
      </h2>
      {error !== null && <ErrorMessage error={error} />}
      <div className="rows">
        {devices?.map((d) => (
          <div key={d.device_id} className={d.active ? "card row" : "card row inactive"}>
            <span className="card-text">
              <span className="card-title">
                {d.recovery ? t("sync.recovery") : d.name}
                {d.this_device && <span className="badge">{t("sync.thisDevice")}</span>}
                {!d.active && <span className="badge danger">{t("sync.revokedMark")}</span>}
                {d.admin_blocked && <span className="badge danger">{t("sync.blockedMark")}</span>}
              </span>
              <span className="card-sub">{[d.platform, seen(d.last_seen)].filter(Boolean).join(" · ")}</span>
            </span>
            {d.active && !d.this_device && !d.recovery && (
              <button className="danger" onClick={() => setRevoking(d)}>
                {t("sync.revoke")}
              </button>
            )}
          </div>
        ))}
      </div>
      <div className="buttons">
        <button onClick={onClose}>{t("common.close")}</button>
      </div>
      {revoking && (
        <ConfirmDialog
          message={t("sync.revokeConfirm", { name: revoking.name })}
          confirmLabel={t("sync.revoke")}
          onConfirm={() => void revoke(revoking)}
          onCancel={() => setRevoking(null)}
        />
      )}
    </div>
  );
}
