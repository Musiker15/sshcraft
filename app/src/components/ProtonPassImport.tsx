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
        setStatus("not_installed");
        setMessage(errorText(t, e));
      },
    );
  }, []);

  async function importOne(key: PassKey) {
    setBusy(key.item_id);
    setMessage(null);
    try {
      await importFromPassCli(key.share_id, key.item_id, key.title);
      setMessage(`„${key.title}“ wurde in den Tresor übernommen.`);
      onImported();
    } catch (e) {
      setMessage(errorText(t, e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <h2>Aus Proton Pass</h2>
      <p className="empty" style={{ margin: 0 }}>
        Zum Verbinden musst du nichts importieren: SSHCraft nutzt den Proton-Pass-Agenten direkt. Importieren brauchst du
        nur, wenn der Schlüssel später auch synchronisiert werden soll.
      </p>
      {status === "loading" && <div>Prüfe pass-cli …</div>}
      {status === "not_installed" && (
        <div>
          pass-cli ist nicht installiert. Anleitung: protonpass.github.io/pass-cli unter „Get started“.
        </div>
      )}
      {status === "logged_out" && <div>pass-cli ist nicht angemeldet. Melde dich im Terminal mit `pass-cli login` an.</div>}
      {status === "ready" && keys.length === 0 && <div>Keine SSH-Schlüssel in Proton Pass gefunden.</div>}
      {status === "ready" &&
        keys.map((k) => (
          <div key={k.item_id} className="key-row">
            <div className="head">
              <strong>{k.title}</strong>
              <button disabled={busy !== null} onClick={() => void importOne(k)}>
                {busy === k.item_id ? "Importiere …" : "Importieren"}
              </button>
            </div>
          </div>
        ))}
      {message && <div>{message}</div>}
    </>
  );
}
