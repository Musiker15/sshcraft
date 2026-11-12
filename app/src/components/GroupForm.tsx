import { useState } from "react";
import { saveGroup, type Group, type KeyInfo } from "../api";
import { errorText, FormError } from "../i18n/errors";
import { useLanguage, useT } from "../i18n/i18n";
import { fromConnectionSettings, toConnectionSettings } from "../forms/hostForm";
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
  onDone: () => void;
  onCancel: () => void;
};

export function GroupForm({ group, groups, parent, keys, onDone, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [name, setName] = useState(group?.name ?? "");
  const [parentId, setParentId] = useState(group?.parent_id ?? parent ?? "");
  const [tags, setTags] = useState(group?.tags ?? []);
  const [values, setValues] = useState(
    fromConnectionSettings(group?.defaults ?? { address: null, port: null, username: null, key: null }),
  );
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
      await saveGroup({
        ...group,
        id: group?.id ?? crypto.randomUUID(),
        name: name.trim(),
        defaults: toConnectionSettings(values),
        parent_id: parentId || null,
        tags,
      });
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
