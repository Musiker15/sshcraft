import { useState } from "react";
import { createSyncAccount, joinSyncAccount, type AccountCreated } from "../../api";
import { useT } from "../../i18n/i18n";
import { ErrorMessage } from "../../shell/ErrorMessage";

type Mode = "create" | "join";
type Props = {
  onCreated: (created: AccountCreated) => void;
  onJoined: () => void;
  onCancel: () => void;
};

/** Sets up a sync server: a new account, or this device joining one with the account key. */
export function AddServerForm({ onCreated, onJoined, onCancel }: Props) {
  const t = useT();
  const [mode, setMode] = useState<Mode>("create");
  const [serverName, setServerName] = useState("");
  const [origin, setOrigin] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [accountKey, setAccountKey] = useState("");
  const [deviceName, setDeviceName] = useState(navigator.userAgent.includes("Windows") ? "Windows" : "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [missing, setMissing] = useState(false);

  const filled = (...values: string[]) => values.every((v) => v.trim() !== "");

  async function submit() {
    const complete =
      mode === "create"
        ? filled(serverName, origin, apiKey, name, email, deviceName)
        : filled(serverName, origin, apiKey, accountKey, deviceName);
    setMissing(!complete);
    if (!complete) return;
    setBusy(true);
    setError(null);
    try {
      const common = {
        server_name: serverName.trim(),
        origin: origin.trim(),
        api_key: apiKey.trim(),
        device_name: deviceName.trim(),
      };
      if (mode === "create") {
        onCreated(await createSyncAccount({ ...common, name: name.trim(), email: email.trim() }));
      } else {
        await joinSyncAccount({ ...common, account_key: accountKey });
        onJoined();
      }
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  const field = (label: string, value: string, set: (v: string) => void, type = "text") => (
    <label>
      {label}
      <input type={type} value={value} onChange={(e) => set(e.target.value)} autoComplete="off" />
    </label>
  );

  return (
    <div className="dialog">
      <h2>{t("sync.add")}</h2>
      <div className="segmented-choice" role="radiogroup">
        <label>
          <input type="radio" checked={mode === "create"} onChange={() => setMode("create")} />
          {t("sync.modeCreate")}
        </label>
        <label>
          <input type="radio" checked={mode === "join"} onChange={() => setMode("join")} />
          {t("sync.modeJoin")}
        </label>
      </div>
      {field(t("sync.serverName"), serverName, setServerName)}
      {field(t("sync.origin"), origin, setOrigin, "url")}
      {field(t("sync.apiKey"), apiKey, setApiKey, "password")}
      {mode === "create" ? (
        <>
          {field(t("sync.name"), name, setName)}
          {field(t("sync.email"), email, setEmail, "email")}
        </>
      ) : (
        field(t("sync.accountKey"), accountKey, setAccountKey, "password")
      )}
      {field(t("sync.deviceName"), deviceName, setDeviceName)}
      {missing && <div className="error">{t("sync.required")}</div>}
      {error !== null && <ErrorMessage error={error} />}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={busy} onClick={() => void submit()}>
          {busy ? t("sync.working") : mode === "create" ? t("sync.create") : t("sync.join")}
        </button>
      </div>
    </div>
  );
}
