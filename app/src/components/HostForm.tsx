import { useState } from "react";
import { forgetHostKey, HOST_OS, saveHost, type Group, type Host, type HostOs, type KeyInfo, applyPassword, type Identity, type PasswordChange } from "../api";
import { errorText, FormError } from "../i18n/errors";
import { useLanguage, useT } from "../i18n/i18n";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { emptyConnection, fromConnectionOptions, toConnectionOptions } from "../forms/connectionForm";
import { ConnectionFields } from "./ConnectionFields";
import { groupLabel, inheritedDefaults } from "../pages/hostsTree";
import { SettingsFields } from "./SettingsFields";
import { TagInput } from "./TagInput";

type Props = {
  host?: Host;
  groups: Group[];
  /** Group of a new host. */
  group?: string | null;
  keys: KeyInfo[];
  identities?: Identity[];
  hosts?: Host[];
  onDone: () => void;
  onCancel: () => void;
};

export function HostForm({ host, groups, group: initialGroup, keys, identities = [], hosts = [], onDone, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [name, setName] = useState(host?.name ?? "");
  const [groupId, setGroupId] = useState(host?.group_id ?? initialGroup ?? "");
  const [tags, setTags] = useState(host?.tags ?? []);
  const [os, setOs] = useState<HostOs | "">(host?.os ?? "");
  const [values, setValues] = useState(
    fromConnectionSettings(host?.settings ?? { address: null, port: null, username: null, key: null }),
  );
  const [connection, setConnection] = useState(host ? fromConnectionOptions(host.settings) : emptyConnection);
  const [password, setPassword] = useState<PasswordChange>({ kind: "keep" });
  const [proxyPassword, setProxyPassword] = useState<PasswordChange>({ kind: "keep" });
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const options = groups
    .map((g) => ({ id: g.id, label: groupLabel(groups, g.id) }))
    .sort((a, b) => a.label.localeCompare(b.label, lang));

  async function submit() {
    try {
      if (!name.trim()) throw new FormError("name_required");
      const id = host?.id ?? crypto.randomUUID();
      const settings = { ...toConnectionSettings(values), ...toConnectionOptions(connection) };
      await saveHost({
        ...host,
        id,
        name: name.trim(),
        group_id: groupId || null,
        settings,
        tags,
        os: os || null,
      });
      // Passwords go after the host exists (and has its proxy).
      await applyPassword({ kind: "host", id }, password);
      if (settings.proxy && settings.proxy.kind !== "direct") {
        await applyPassword({ kind: "host_proxy", id }, proxyPassword);
      }
      onDone();
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  async function forget() {
    if (!host) return;
    try {
      await forgetHostKey(host.id);
      setNotice(t("hostForm.forgotten"));
    } catch (e) {
      setError(errorText(t, e));
    }
  }

  return (
    <div className="dialog">
      <h2>{host ? t("hostForm.editTitle") : t("hostForm.newTitle")}</h2>
      <label>
        {t("form.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label>
        {t("hostForm.group")}
        <select value={groupId} onChange={(e) => setGroupId(e.target.value)}>
          <option value="">{t("hostForm.noGroup")}</option>
          {options.map((g) => (
            <option key={g.id} value={g.id}>
              {g.label}
            </option>
          ))}
        </select>
      </label>
      <TagInput value={tags} onChange={setTags} />
      <label>
        {t("hostForm.os")}
        <select value={os} onChange={(e) => setOs(e.target.value as HostOs | "")}>
          <option value="">{t("hostForm.osAuto")}</option>
          {HOST_OS.map((o) => (
            <option key={o} value={o}>
              {t(`os.${o}`)}
            </option>
          ))}
        </select>
      </label>
      <SettingsFields
        values={values}
        onChange={setValues}
        keys={keys}
        inherited={groupId ? inheritedDefaults(groups, groupId) : undefined}
      />
      <ConnectionFields
        values={connection}
        onChange={setConnection}
        identities={identities}
        hosts={hosts.filter((h) => h.id !== host?.id)}
        password={password}
        onPassword={setPassword}
        passwordStored={host?.has_password ?? false}
        proxyPassword={proxyPassword}
        onProxyPassword={setProxyPassword}
        proxyPasswordStored={host?.proxy_has_password ?? false}
      />
      {host && (
        <div>
          <button onClick={forget}>{t("hostForm.forget")}</button>
        </div>
      )}
      {notice && <div>{notice}</div>}
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
