import { useState } from "react";
import { forgetHostKey, saveHost, type Group, type Host, type KeyInfo } from "../api";
import { errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { SettingsFields } from "./SettingsFields";

type Props = { host?: Host; groups: Group[]; keys: KeyInfo[]; onDone: () => void; onCancel: () => void };

export function HostForm({ host, groups, keys, onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState(host?.name ?? "");
  const [groupId, setGroupId] = useState(host?.group_id ?? "");
  const [values, setValues] = useState(
    fromConnectionSettings(host?.settings ?? { address: null, port: null, username: null, key: null }),
  );
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const group = groups.find((g) => g.id === groupId);

  async function submit() {
    try {
      if (!name.trim()) throw new Error("Bitte einen Namen eingeben.");
      await saveHost({
        id: host?.id ?? crypto.randomUUID(),
        name: name.trim(),
        group_id: groupId || null,
        settings: toConnectionSettings(values),
      });
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  async function forget() {
    if (!host) return;
    try {
      await forgetHostKey(host.id);
      setNotice("Der gespeicherte Fingerprint ist vergessen. Beim nächsten Verbinden wirst du gefragt.");
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  return (
    <div className="dialog">
      <h2>{host ? "Host bearbeiten" : "Neuer Host"}</h2>
      <label>
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label>
        Gruppe
        <select value={groupId} onChange={(e) => setGroupId(e.target.value)}>
          <option value="">Keine Gruppe</option>
          {groups.map((g) => (
            <option key={g.id} value={g.id}>
              {g.name}
            </option>
          ))}
        </select>
      </label>
      <SettingsFields values={values} onChange={setValues} keys={keys} inherited={group?.defaults} />
      {host && (
        <div>
          <button onClick={forget}>Fingerprint vergessen</button>
        </div>
      )}
      {notice && <div>{notice}</div>}
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>Abbrechen</button>
        <button className="primary" onClick={submit}>
          Speichern
        </button>
      </div>
    </div>
  );
}
