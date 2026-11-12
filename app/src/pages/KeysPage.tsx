import { KeyRound, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { deleteKey, listAgentKeys, type AgentKey, type KeyInfo, type ServerView } from "../api";
import { KeyGenerateForm, KeyImportForm } from "../components/KeyForms";
import { ProtonPassImport } from "../components/ProtonPassImport";
import { useLanguage, useT } from "../i18n/i18n";
import { ConfirmDialog } from "../shell/ConfirmDialog";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";
import { Drawer } from "../shell/Drawer";
import { ErrorMessage } from "../shell/ErrorMessage";
import { sortByName } from "./hostsView";

type Panel = "import" | "generate" | "protonpass" | null;
type Menu = { items: MenuItem[]; at: { x: number; y: number } };

/** Keys in this vault (with secret) and keys the Proton Pass agent offers (never copied). */
type Props = {
  keys: KeyInfo[];
  onChanged: () => void;
  /** Sync servers; a key can move only to one that syncs keys. */
  servers?: ServerView[];
  onMove?: (id: string, location: string | null) => void;
};

export function KeysPage({ keys, onChanged, servers = [], onMove }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [panel, setPanel] = useState<Panel>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [deleting, setDeleting] = useState<KeyInfo | null>(null);
  const [problem, setProblem] = useState<unknown>(null);
  const [agentKeys, setAgentKeys] = useState<AgentKey[] | null | "loading">("loading");

  const loadAgent = useCallback(() => {
    listAgentKeys().then(setAgentKeys, () => setAgentKeys(null));
  }, []);
  useEffect(loadAgent, [loadAgent]);

  const closePanel = useCallback(() => setPanel(null), []);
  const done = () => {
    setPanel(null);
    onChanged();
  };

  async function remove(key: KeyInfo) {
    setDeleting(null);
    try {
      await deleteKey(key.id);
      onChanged();
    } catch (e) {
      setProblem(e);
    }
  }

  const serverName = (id: string | null | undefined) => servers.find((s) => s.id === id)?.name;
  /** "Move to": this device only, or a server that syncs keys; not where the key is now. */
  const moveItems = (key: KeyInfo) => {
    if (!onMove) return [];
    const places: { id: string | null; name: string }[] = [
      { id: null, name: t("sync.localOnly") },
      ...servers.filter((s) => s.sync_keys && !s.revoked).map((s) => ({ id: s.id, name: s.name })),
    ];
    return places
      .filter((p) => p.id !== (key.location ?? null))
      .filter((p) => p.id !== null || key.location)
      .map((p) => ({ label: t("sync.moveTo", { place: p.name }), onSelect: () => onMove(key.id, p.id) }));
  };

  return (
    <section className="page">
      <header className="page-head">
        <h1>{t("nav.keys")}</h1>
        <div className="page-actions">
          <button onClick={() => setPanel("protonpass")}>{t("keys.fromProtonPass")}</button>
          <button onClick={() => setPanel("import")}>{t("keys.import")}</button>
          <button className="primary" onClick={() => setPanel("generate")}>
            {t("keys.generate")}
          </button>
        </div>
      </header>
      {problem !== null && <ErrorMessage error={problem} />}

      <div className="group-block">
        <h2 className="group-title">{t("keys.vault")}</h2>
        {keys.length === 0 && <p className="empty">{t("keys.vaultEmpty")}</p>}
        <div className="cards">
          {sortByName(keys, lang).map((key) => (
            <div
              key={key.id}
              className="card"
              tabIndex={0}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({
                  at: { x: e.clientX, y: e.clientY },
                  items: [
                    { label: t("keys.copyPublic"), onSelect: () => void navigator.clipboard.writeText(key.public_key) },
                    ...moveItems(key),
                    { label: t("common.delete"), onSelect: () => setDeleting(key), danger: true },
                  ],
                });
              }}
            >
              <span className="card-icon">
                <KeyRound size={18} aria-hidden="true" />
              </span>
              <span className="card-text">
                <span className="card-title">
                  {key.name}
                  {serverName(key.location) && <span className="badge">{serverName(key.location)}</span>}
                </span>
                <span className="card-sub">{key.algorithm}</span>
                <span className="card-sub mono">{key.fingerprint}</span>
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="group-block">
        <h2 className="group-title">{t("keys.agent")}</h2>
        {agentKeys === null && <p className="empty">{t("errors.agent_unavailable")}</p>}
        {Array.isArray(agentKeys) && agentKeys.length === 0 && <p className="empty">{t("keys.agentEmpty")}</p>}
        <div className="cards">
          {Array.isArray(agentKeys) &&
            agentKeys.map((key) => (
              <div key={key.fingerprint} className="card" tabIndex={0}>
                <span className="card-icon group">
                  <ShieldCheck size={18} aria-hidden="true" />
                </span>
                <span className="card-text">
                  <span className="card-title">{key.comment || key.fingerprint}</span>
                  <span className="card-sub">{key.algorithm}</span>
                  <span className="card-sub mono">{key.fingerprint}</span>
                </span>
              </div>
            ))}
        </div>
      </div>

      {panel && (
        <Drawer onClose={closePanel}>
          {panel === "import" && <KeyImportForm onDone={done} onCancel={closePanel} />}
          {panel === "generate" && <KeyGenerateForm onDone={done} onCancel={closePanel} />}
          {panel === "protonpass" && (
            <div className="dialog">
              <ProtonPassImport onImported={onChanged} />
              <div className="buttons">
                <button onClick={closePanel}>{t("common.close")}</button>
              </div>
            </div>
          )}
        </Drawer>
      )}
      {menu && <ContextMenu items={menu.items} at={menu.at} onClose={() => setMenu(null)} />}
      {deleting && (
        <ConfirmDialog
          message={t("keys.deleteConfirm", { name: deleting.name })}
          confirmLabel={t("common.delete")}
          onConfirm={() => void remove(deleting)}
          onCancel={() => setDeleting(null)}
        />
      )}
    </section>
  );
}
