import { useState } from "react";
import { saveGroup, type Group, type KeyInfo } from "../api";
import { errorText, FormError } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { SettingsFields } from "./SettingsFields";

type Props = { group?: Group; keys: KeyInfo[]; onDone: () => void; onCancel: () => void };

export function GroupForm({ group, keys, onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState(group?.name ?? "");
  const [values, setValues] = useState(
    fromConnectionSettings(group?.defaults ?? { address: null, port: null, username: null, key: null }),
  );
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    try {
      if (!name.trim()) throw new FormError("name_required");
      await saveGroup({ id: group?.id ?? crypto.randomUUID(), name: name.trim(), defaults: toConnectionSettings(values) });
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  return (
    <div className="dialog">
      <h2>{group ? t("groupForm.editTitle") : t("groupForm.newTitle")}</h2>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <p className="hint">{t("groupForm.hint")}</p>
      <SettingsFields values={values} onChange={setValues} keys={keys} />
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" onClick={submit}>
          {t("common.save")}
        </button>
      </div>
    </div>
  );
}
