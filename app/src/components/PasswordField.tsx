import { useState } from "react";
import type { PasswordChange } from "../api";
import { useT } from "../i18n/i18n";

type Props = {
  label: string;
  /** Whether a password is stored. The field never gets the password itself. */
  stored: boolean;
  value: PasswordChange;
  onChange: (change: PasswordChange) => void;
};

/** A stored password shows as "stored" with change and remove; otherwise an input. */
export function PasswordField({ label, stored, value, onChange }: Props) {
  const t = useT();
  const [editing, setEditing] = useState(!stored);

  if (stored && value.kind === "clear") {
    return (
      <div className="password-field">
        <span className="field-label">{label}</span>
        <span className="hint">{t("password.removing")}</span>
        <button onClick={() => onChange({ kind: "keep" })}>{t("password.undo")}</button>
      </div>
    );
  }
  if (stored && !editing) {
    return (
      <div className="password-field">
        <span className="field-label">{label}</span>
        <span className="hint">{t("password.stored")}</span>
        <div className="buttons">
          <button
            onClick={() => {
              setEditing(true);
              onChange({ kind: "set", secret: "" });
            }}
          >
            {t("password.change")}
          </button>
          <button className="danger" onClick={() => onChange({ kind: "clear" })}>
            {t("password.remove")}
          </button>
        </div>
      </div>
    );
  }
  return (
    <label>
      {label}
      <input
        type="password"
        autoComplete="new-password"
        value={value.kind === "set" ? value.secret : ""}
        placeholder={t("password.placeholder")}
        onChange={(e) => onChange(e.target.value === "" && !stored ? { kind: "keep" } : { kind: "set", secret: e.target.value })}
      />
    </label>
  );
}
