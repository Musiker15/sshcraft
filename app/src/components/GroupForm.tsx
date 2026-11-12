import { useState } from "react";
import { saveGroup, type Group, type KeyInfo, applyPassword, type Host, type Identity, type PasswordChange } from "../api";
import { errorText, FormError } from "../i18n/errors";
import { useLanguage, useT } from "../i18n/i18n";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
import { emptyConnection, fromConnectionOptions, toConnectionOptions } from "../forms/connectionForm";
import { ConnectionFields } from "./ConnectionFields";
import { descendants, groupLabel, inheritedDefaults } from "../pages/hostsTree";
import { SettingsFields } from "./SettingsFields";
import { TagInput } from "./TagInput";

type Props = {
  group?: Group;
  /** Every group, for the parent choice and inherited values. */
  groups: Group[];
  /** Parent of a new group. */
  parent?: string | null;
  keys: KeyInfo[];
  identities?: Identity[];
  hosts?: Host[];
  onDone: () => void;
  onCancel: () => void;
};

export function GroupForm({ group, groups, parent, keys, identities = [], hosts = [], onDone, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [name, setName] = useState(group?.name ?? "");
  const [parentId, setParentId] = useState(group?.parent_id ?? parent ?? "");
  const [tags, setTags] = useState(group?.tags ?? []);
  const [values, setValues] = useState(
    fromConnectionSettings(group?.defaults ?? { address: null, port: null, username: null, key: null }),
  );
  const [connection, setConnection] = useState(group ? fromConnectionOptions(group.defaults) : emptyConnection);
  const [password, setPassword] = useState<PasswordChange>({ kind: "keep" });
  const [proxyPassword, setProxyPassword] = useState<PasswordChange>({ kind: "keep" });
  const [error, setError] = useState<string | null>(null);
  // A group cannot go into itself or one of its subgroups.
  const blocked = group ? descendants(groups, group.id) : new Set<string>();
  const parents = groups
    .filter((g) => g.id !== group?.id && !blocked.has(g.id))
    .map((g) => ({ id: g.id, label: groupLabel(groups, g.id) }))
    .sort((a, b) => a.label.localeCompare(b.label, lang));

  async function submit() {
    try {
      if (!name.trim()) throw new FormError("name_required");
      const id = group?.id ?? crypto.randomUUID();
      const defaults = { ...toConnectionSettings(values), ...toConnectionOptions(connection) };
      await saveGroup({
        ...group,
        id,
        name: name.trim(),
        defaults,
        parent_id: parentId || null,
        tags,
      });
      await applyPassword({ kind: "group", id }, password);
      if (defaults.proxy && defaults.proxy.kind !== "direct") {
        await applyPassword({ kind: "group_proxy", id }, proxyPassword);
      }
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
      <label>
        {t("groupForm.parent")}
        <select value={parentId} onChange={(e) => setParentId(e.target.value)}>
          <option value="">{t("groupForm.noParent")}</option>
          {parents.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
      </label>
      <TagInput value={tags} onChange={setTags} />
      <p className="hint">{t("groupForm.hint")}</p>
      <SettingsFields
        values={values}
        onChange={setValues}
        keys={keys}
        inherited={parentId ? inheritedDefaults(groups, parentId) : undefined}
      />
      <ConnectionFields
        values={connection}
        onChange={setConnection}
        identities={identities}
        hosts={hosts}
        password={password}
        onPassword={setPassword}
        passwordStored={group?.has_password ?? false}
        proxyPassword={proxyPassword}
        onProxyPassword={setProxyPassword}
        proxyPasswordStored={group?.proxy_has_password ?? false}
      />
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
