import { useState } from "react";
import {
  attachCertificate,
  installKey,
  protectKey,
  rememberPassphrase,
  type AgentKey,
  type Host,
  type KeyInfo,
  type KeyRef,
} from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

/** Saves the passphrase of a key (`remember`) or stops saving it (`protect`). */
export function PassphraseDialog({
  keyInfo,
  mode,
  onDone,
  onCancel,
}: {
  keyInfo: KeyInfo;
  mode: "remember" | "protect";
  onDone: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mismatch = mode === "protect" && passphrase !== repeat;

  async function run() {
    setBusy(true);
    setError(null);
    try {
      if (mode === "remember") await rememberPassphrase(keyInfo.id, passphrase);
      else await protectKey(keyInfo.id, passphrase);
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog">
      <h2>{t(mode === "remember" ? "keys.rememberTitle" : "keys.protectTitle", { name: keyInfo.name })}</h2>
      <p className="hint">{t(mode === "remember" ? "keys.rememberHint" : "keys.protectHint")}</p>
      <label>
        {t("keys.passphrase")}
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoFocus />
      </label>
      {mode === "protect" && (
        <label>
          {t("keys.repeatPassphrase")}
          <input type="password" value={repeat} onChange={(e) => setRepeat(e.target.value)} />
        </label>
      )}
      {mismatch && repeat && <div className="error">{t("errors.passphrase_mismatch")}</div>}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={busy || !passphrase || mismatch} onClick={() => void run()}>
          {t("common.save")}
        </button>
      </div>
    </div>
  );
}

/** Attaches an OpenSSH user certificate to a key: pasted or from a `-cert.pub` file. */
export function CertificateDialog({
  keyInfo,
  onDone,
  onCancel,
}: {
  keyInfo: KeyInfo;
  onDone: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run() {
    setBusy(true);
    setError(null);
    try {
      await attachCertificate(keyInfo.id, text);
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog">
      <h2>{t("keys.certificateTitle", { name: keyInfo.name })}</h2>
      <label>
        {t("keys.certificateText")}
        <textarea value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} autoFocus />
      </label>
      <input
        type="file"
        accept=".pub"
        aria-label={t("keys.certificateFile")}
        onChange={(e) => void e.target.files?.[0]?.text().then(setText)}
      />
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={busy || !text.trim()} onClick={() => void run()}>
          {t("common.save")}
        </button>
      </div>
    </div>
  );
}

/** A key choice as one select value: "stored:" plus the id, or "agent:" plus the fingerprint. */
function refValue(key: KeyRef): string {
  return key.kind === "stored" ? `stored:${key.id}` : `agent:${key.fingerprint}`;
}

function parseRef(value: string): KeyRef | null {
  if (value.startsWith("stored:")) return { kind: "stored", id: value.slice(7) };
  if (value.startsWith("agent:")) return { kind: "agent", fingerprint: value.slice(6) };
  return null;
}

/** Puts a public key into `authorized_keys` on a host. Either side may be chosen beforehand. */
export function InstallKeyDialog({
  hosts,
  keys,
  agentKeys = [],
  host,
  keyRef,
  onClose,
}: {
  hosts: Host[];
  keys: KeyInfo[];
  agentKeys?: AgentKey[];
  host?: string;
  keyRef?: KeyRef;
  onClose: () => void;
}) {
  const t = useT();
  const [hostId, setHostId] = useState(host ?? hosts[0]?.id ?? "");
  const [key, setKey] = useState(keyRef ? refValue(keyRef) : keys[0] ? `stored:${keys[0].id}` : "");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run() {
    const ref = parseRef(key);
    if (!ref || !hostId) return;
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const outcome = await installKey(hostId, ref);
      setResult(t(outcome === "added" ? "keys.installAdded" : "keys.installPresent"));
    } catch (e) {
      setError(errorText(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog">
      <h2>{t("keys.installTitle")}</h2>
      <p className="hint">{t("keys.installHint")}</p>
      <label>
        {t("keys.installHost")}
        <select value={hostId} onChange={(e) => setHostId(e.target.value)}>
          {hosts.map((h) => (
            <option key={h.id} value={h.id}>
              {h.name}
            </option>
          ))}
        </select>
      </label>
      <label>
        {t("keys.installKey")}
        <select value={key} onChange={(e) => setKey(e.target.value)}>
          {keys.map((k) => (
            <option key={k.id} value={`stored:${k.id}`}>
              {k.name}
            </option>
          ))}
          {agentKeys.map((k) => (
            <option key={k.fingerprint} value={`agent:${k.fingerprint}`}>
              {k.comment || k.fingerprint}
            </option>
          ))}
        </select>
      </label>
      {result && <p className="hint">{result}</p>}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onClose}>{t("common.close")}</button>
        <button className="primary" disabled={busy || !hostId || !key} onClick={() => void run()}>
          {t("keys.installRun")}
        </button>
      </div>
    </div>
  );
}
