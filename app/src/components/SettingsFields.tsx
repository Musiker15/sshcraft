import { useEffect, useState } from "react";
import { listAgentKeys, type AgentKey, type ConnectionSettings, type KeyInfo } from "../api";
import { keyChoice, type SettingsFormValues } from "../forms/hostForm";
import { useT } from "../i18n/i18n";

type Props = {
  values: SettingsFormValues;
  onChange: (values: SettingsFormValues) => void;
  keys: KeyInfo[];
  /** Values a host inherits from its group, shown as placeholders. */
  inherited?: ConnectionSettings;
  /** Only user and key (an identity has no address). */
  credentialsOnly?: boolean;
};

/** Address, port, user and key fields, shared by the host and group forms. */
export function SettingsFields({ values, onChange, keys, inherited, credentialsOnly }: Props) {
  const t = useT();
  const [agentKeys, setAgentKeys] = useState<AgentKey[] | null>(null);
  useEffect(() => {
    listAgentKeys().then(setAgentKeys, () => setAgentKeys(null));
  }, []);

  const set = (field: keyof SettingsFormValues) => (value: string) => onChange({ ...values, [field]: value });
  const label = (choice: string): string | null => {
    const stored = keys.find((k) => `stored:${k.id}` === choice);
    if (stored) return stored.name;
    const agent = agentKeys?.find((k) => `agent:${k.fingerprint}` === choice);
    if (agent) return t("settingsFields.agentKey", { name: agent.comment || agent.fingerprint });
    return null;
  };
  const inheritedChoice = keyChoice(inherited?.key ?? null);
  const inheritedLabel = inheritedChoice ? (label(inheritedChoice) ?? t("settingsFields.notFound")) : null;
  const currentMissing = values.keyChoice !== "" && label(values.keyChoice) === null;

  return (
    <>
      {!credentialsOnly && (
      <div className="grid-2">
        <label>
          {t("settingsFields.address")}
          <input
            value={values.address}
            placeholder={inherited?.address ?? "server.example.org"}
            onChange={(e) => set("address")(e.target.value)}
          />
        </label>
        <label>
          {t("settingsFields.port")}
          <input
            value={values.port}
            placeholder={String(inherited?.port ?? 22)}
            inputMode="numeric"
            onChange={(e) => set("port")(e.target.value)}
          />
        </label>
      </div>
      )}
      <label>
        {t("settingsFields.user")}
        <input
          value={values.username}
          placeholder={inherited?.username ?? "root"}
          onChange={(e) => set("username")(e.target.value)}
        />
      </label>
      <label>
        {t("settingsFields.key")}
        <select value={values.keyChoice} onChange={(e) => set("keyChoice")(e.target.value)}>
          <option value="">{inheritedLabel ? t("settingsFields.fromGroup", { name: inheritedLabel }) : t("settingsFields.none")}</option>
          {currentMissing && <option value={values.keyChoice}>{t("settingsFields.missing", { choice: values.keyChoice })}</option>}
          <optgroup label={t("settingsFields.groupAgent")}>
            {agentKeys === null && <option disabled>{t("settingsFields.agentUnavailable")}</option>}
            {agentKeys?.map((k) => (
              <option key={k.fingerprint} value={`agent:${k.fingerprint}`}>
                {k.comment || k.fingerprint} ({k.algorithm})
              </option>
            ))}
          </optgroup>
          <optgroup label={t("settingsFields.groupStored")}>
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
