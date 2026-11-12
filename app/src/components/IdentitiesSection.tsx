import { UserRound } from "lucide-react";
import { useState } from "react";
import { identityDelete, type Identity, type KeyInfo, type ServerView } from "../api";
import { useT } from "../i18n/i18n";
import { ConfirmDialog } from "../shell/ConfirmDialog";
import { Drawer } from "../shell/Drawer";
import { ErrorMessage } from "../shell/ErrorMessage";
import { IdentityForm } from "./IdentityForm";

type Props = {
  identities: Identity[];
  keys: KeyInfo[];
  servers: ServerView[];
  onChanged: () => void;
};

/** Identities on the keys page: list, create, edit, delete. */
export function IdentitiesSection({ identities, keys, servers, onChanged }: Props) {
  const t = useT();
  const [editing, setEditing] = useState<Identity | "new" | null>(null);
  const [deleting, setDeleting] = useState<Identity | null>(null);
  const [problem, setProblem] = useState<unknown>(null);
  const done = () => {
    setEditing(null);
    onChanged();
  };

  async function remove(identity: Identity) {
    setDeleting(null);
    try {
      await identityDelete(identity.id);
      onChanged();
    } catch (e) {
      setProblem(e);
    }
  }

  return (
    <div className="group-block">
      <div className="group-title-row">
        <h2 className="group-title">{t("identity.title")}</h2>
        <button onClick={() => setEditing("new")}>{t("identity.new")}</button>
      </div>
      {problem != null && <ErrorMessage error={problem} />}
      {identities.length === 0 && <p className="empty">{t("identity.empty")}</p>}
      <div className="cards">
        {identities.map((identity) => (
          <div key={identity.id} className="card" tabIndex={0} onDoubleClick={() => setEditing(identity)}>
            <span className="card-icon group">
              <UserRound size={18} aria-hidden="true" />
            </span>
            <span className="card-text">
              <span className="card-title">{identity.name}</span>
              <span className="card-sub">
                {[identity.username, identity.has_password ? t("identity.hasPassword") : null]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </span>
            <span className="card-actions">
              <button onClick={() => setEditing(identity)}>{t("identity.editShort")}</button>
              <button className="danger" onClick={() => setDeleting(identity)}>
                {t("common.delete")}
              </button>
            </span>
          </div>
        ))}
      </div>
      {editing && (
        <Drawer onClose={() => setEditing(null)}>
          <IdentityForm
            identity={editing === "new" ? undefined : editing}
            keys={keys}
            servers={servers}
            onDone={done}
            onCancel={() => setEditing(null)}
          />
        </Drawer>
      )}
      {deleting && (
        <ConfirmDialog
          message={t("identity.deleteConfirm", { name: deleting.name })}
          confirmLabel={t("common.delete")}
          onConfirm={() => void remove(deleting)}
          onCancel={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
