import { useState } from "react";
import { errorMessage, saveGroup, type Group, type KeyInfo } from "../api";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { SettingsFields } from "./SettingsFields";

type Props = { group?: Group; keys: KeyInfo[]; onDone: () => void; onCancel: () => void };

export function GroupForm({ group, keys, onDone, onCancel }: Props) {
  const [name, setName] = useState(group?.name ?? "");
  const [values, setValues] = useState(
    fromConnectionSettings(group?.defaults ?? { address: null, port: null, username: null, key: null }),
  );
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    try {
      if (!name.trim()) throw new Error("Bitte einen Namen eingeben.");
      await saveGroup({ id: group?.id ?? crypto.randomUUID(), name: name.trim(), defaults: toConnectionSettings(values) });
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="dialog">
      <h2>{group ? "Gruppe bearbeiten" : "Neue Gruppe"}</h2>
      <label>
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <p className="empty" style={{ margin: 0 }}>
        Diese Werte gelten für alle Hosts der Gruppe, solange ein Host sie nicht selbst setzt.
      </p>
      <SettingsFields values={values} onChange={setValues} keys={keys} />
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
