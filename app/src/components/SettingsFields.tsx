import { useEffect, useState } from "react";
import { listAgentKeys, type AgentKey, type ConnectionSettings, type KeyInfo } from "../api";
import { keyChoice, type SettingsFormValues } from "../forms/hostForm";

type Props = {
  values: SettingsFormValues;
  onChange: (values: SettingsFormValues) => void;
  keys: KeyInfo[];
  /** Values a host inherits from its group, shown as placeholders. */
  inherited?: ConnectionSettings;
};

/** Address, port, user and key fields, shared by the host and group forms. */
export function SettingsFields({ values, onChange, keys, inherited }: Props) {
  const [agentKeys, setAgentKeys] = useState<AgentKey[] | null>(null);
  useEffect(() => {
    listAgentKeys().then(setAgentKeys, () => setAgentKeys(null));
  }, []);

  const set = (field: keyof SettingsFormValues) => (value: string) => onChange({ ...values, [field]: value });
  const label = (choice: string): string | null => {
    const stored = keys.find((k) => `stored:${k.id}` === choice);
    if (stored) return stored.name;
    const agent = agentKeys?.find((k) => `agent:${k.fingerprint}` === choice);
    if (agent) return `${agent.comment || agent.fingerprint} (Proton Pass)`;
    return null;
  };
  const inheritedChoice = keyChoice(inherited?.key ?? null);
  const inheritedLabel = inheritedChoice ? (label(inheritedChoice) ?? "nicht gefunden") : null;
  const currentMissing = values.keyChoice !== "" && label(values.keyChoice) === null;

  return (
    <>
      <div className="grid-2">
        <label>
          Adresse
          <input
            value={values.address}
            placeholder={inherited?.address ?? "server.example.org"}
            onChange={(e) => set("address")(e.target.value)}
          />
        </label>
        <label>
          Port
          <input
            value={values.port}
            placeholder={String(inherited?.port ?? 22)}
            inputMode="numeric"
            onChange={(e) => set("port")(e.target.value)}
          />
        </label>
      </div>
      <label>
        Benutzer
        <input
          value={values.username}
          placeholder={inherited?.username ?? "root"}
          onChange={(e) => set("username")(e.target.value)}
        />
      </label>
      <label>
        Schlüssel
        <select value={values.keyChoice} onChange={(e) => set("keyChoice")(e.target.value)}>
          <option value="">{inheritedLabel ? `Von der Gruppe: ${inheritedLabel}` : "Keiner"}</option>
          {currentMissing && <option value={values.keyChoice}>Nicht gefunden: {values.keyChoice}</option>}
          <optgroup label="Proton Pass">
            {agentKeys === null && <option disabled>Proton Pass nicht erreichbar</option>}
            {agentKeys?.map((k) => (
              <option key={k.fingerprint} value={`agent:${k.fingerprint}`}>
                {k.comment || k.fingerprint} ({k.algorithm})
              </option>
            ))}
          </optgroup>
          <optgroup label="Gespeichert">
            {keys.map((k) => (
              <option key={k.id} value={`stored:${k.id}`}>
                {k.name} ({k.algorithm})
              </option>
            ))}
          </optgroup>
        </select>
      </label>
    </>
  );
}
