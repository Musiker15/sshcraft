import { useEffect, useState } from "react";
import { importFromPassCli, passCliListKeys, passCliStatus, type PassCliStatus, type PassKey } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

/** Lists SSH keys in Proton Pass (via pass-cli) and imports a chosen one into the vault. */
export function ProtonPassImport({ onImported }: { onImported: () => void }) {
  const t = useT();
  const [status, setStatus] = useState<PassCliStatus | "loading">("loading");
  const [keys, setKeys] = useState<PassKey[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    passCliStatus().then(
      (s) => {
        setStatus(s);
        if (s === "ready") passCliListKeys().then(setKeys, (e) => setMessage(errorText(t, e)));
      },
      (e) => {
        setStatus("failed");
        setMessage(errorText(t, e));
      },
    );
  }, [t]);

  async function importOne(key: PassKey) {
    setBusy(key.item_id);
    setMessage(null);
    try {
      await importFromPassCli(key.share_id, key.item_id, key.title);
      setMessage(t("passCli.imported", { name: key.title }));
      onImported();
    } catch (e) {
      setMessage(errorText(t, e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <h2>{t("passCli.title")}</h2>
      <p className="empty">{t("passCli.intro")}</p>
      {status === "loading" && <div>{t("passCli.checking")}</div>}
      {status === "not_installed" && <div>{t("passCli.notInstalled")}</div>}
      {status === "logged_out" && <div>{t("errors.pass_cli_logged_out")}</div>}
      {status === "failed" && <div>{t("passCli.failed")}</div>}
      {status === "ready" && keys.length === 0 && <div>{t("passCli.none")}</div>}
      {status === "ready" &&
        keys.map((k) => (
          <div key={k.item_id} className="key-row">
            <div className="head">
              <strong>{k.title}</strong>
              <button disabled={busy !== null} onClick={() => void importOne(k)}>
                {busy === k.item_id ? t("passCli.importing") : t("passCli.import")}
              </button>
            </div>
          </div>
        ))}
      {message && <div>{message}</div>}
    </>
  );
}
