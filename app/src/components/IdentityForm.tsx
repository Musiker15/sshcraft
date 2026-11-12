import { useState } from "react";
import { identitySave, type Identity, type KeyInfo, type PasswordChange, type ServerView } from "../api";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { errorText, FormError } from "../i18n/errors";
import { useT } from "../i18n/i18n";
import { PasswordField } from "./PasswordField";
import { SettingsFields } from "./SettingsFields";

type Props = {
  identity?: Identity;
  keys: KeyInfo[];
  servers: ServerView[];
  onDone: () => void;
  onCancel: () => void;
};

/** Name, user, key and password of an identity; a new one can go to a sync server. */
export function IdentityForm({ identity, keys, servers, onDone, onCancel }: Props) {
  const t = useT();
  const [name, setName] = useState(identity?.name ?? "");
  const [values, setValues] = useState(
    fromConnectionSettings({
      address: null,
      port: null,
      username: identity?.username ?? null,
      key: identity?.key ?? null,
    }),
  );
  const [password, setPassword] = useState<PasswordChange>({ kind: "keep" });
  const [location, setLocation] = useState(identity?.location ?? "");
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    try {
      if (!name.trim()) throw new FormError("name_required");
      const settings = toConnectionSettings(values);
      await identitySave({
        id: identity?.id ?? null,
        name: name.trim(),
        username: settings.username,
        key: settings.key,
        location: location || null,
        password: password.kind === "set" && password.secret === "" ? { kind: "keep" } : password,
      });
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  return (
    <div className="dialog">
      <h2>{identity ? t("identity.edit") : t("identity.new")}</h2>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <SettingsFields values={values} onChange={setValues} keys={keys} credentialsOnly />
      <PasswordField
        label={t("password.label")}
        stored={identity?.has_password ?? false}
        value={password}
        onChange={setPassword}
      />
      {!identity && servers.length > 0 && (
        <label>
          {t("identity.location")}
          <select value={location} onChange={(e) => setLocation(e.target.value)}>
            <option value="">{t("sync.localOnly")}</option>
            {servers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
      )}
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
