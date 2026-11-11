import type { ConnectionSettings, KeyInfo } from "../api";
import type { SettingsFormValues } from "../forms/hostForm";

type Props = {
  values: SettingsFormValues;
  onChange: (values: SettingsFormValues) => void;
  keys: KeyInfo[];
  /** Values a host inherits from its group, shown as placeholders. */
  inherited?: ConnectionSettings;
};

/** Address, port, user and key fields, shared by the host and group forms. */
export function SettingsFields({ values, onChange, keys, inherited }: Props) {
  const set = (field: keyof SettingsFormValues) => (value: string) => onChange({ ...values, [field]: value });
  const inheritedKey = keys.find((k) => k.id === inherited?.key?.id);
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
        <select value={values.keyId} onChange={(e) => set("keyId")(e.target.value)}>
          <option value="">{inheritedKey ? `Von der Gruppe: ${inheritedKey.name}` : "Keiner"}</option>
          {keys.map((k) => (
            <option key={k.id} value={k.id}>
              {k.name} ({k.algorithm})
            </option>
          ))}
        </select>
      </label>
    </>
  );
}
