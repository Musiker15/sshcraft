import { useCallback, useEffect, useState } from "react";
import {
  listGroups,
  listHosts,
  listShares,
  setShareGrants,
  unshare,
  type Group,
  type Host,
  type RevokeOutcome,
  type Role,
  type ServerView,
  type ShareView,
} from "../../api";
import { useT } from "../../i18n/i18n";
import { ConfirmDialog } from "../../shell/ConfirmDialog";
import { ErrorMessage } from "../../shell/ErrorMessage";

/** The shares of one server: own ones (change roles, take people out, end) and received ones. */
export function SharesPanel({ server, onClose }: { server: ServerView; onClose: () => void }) {
  const t = useT();
  const [shares, setShares] = useState<ShareView[] | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [ending, setEnding] = useState<ShareView | null>(null);
  const [keysHint, setKeysHint] = useState(false);

  const load = useCallback(() => {
    Promise.all([listShares(server.id), listGroups(), listHosts()]).then(([s, g, h]) => {
      setShares(s);
      setGroups(g);
      setHosts(h);
      setError(null);
    }, setError);
  }, [server.id]);
  useEffect(load, [load]);

  async function run(action: Promise<RevokeOutcome>) {
    try {
      const outcome = await action;
      setKeysHint(outcome.keys_were_shared);
      load();
    } catch (e) {
      setError(e);
    }
  }

  const title = (share: ShareView) => {
    const group = groups.find((g) => g.id === share.group)?.name;
    const names = share.hosts.map((id) => hosts.find((h) => h.id === id)?.name).filter(Boolean);
    if (share.kind === "group_all" && group) return t("share.groupAll", { name: group });
    return [group, names.join(", ")].filter(Boolean).join(": ");
  };
  const person = (g: { name: string | null; email: string | null; account: string }) =>
    g.name ?? g.email ?? g.account.slice(0, 12);

  const setRole = (share: ShareView, account: string, role: Role | null) => {
    const grants = share.grants
      .filter((g) => role !== null || g.account !== account)
      .map((g) => ({ account: g.account, role: g.account === account && role ? role : g.role }));
    void run(setShareGrants(server.id, share.area, grants));
  };

  const own = shares?.filter((s) => s.owned) ?? [];
  const received = shares?.filter((s) => !s.owned) ?? [];
  return (
    <div className="dialog">
      <h2>
        {t("share.panel")}: {server.name}
      </h2>
      {error !== null && <ErrorMessage error={error} />}
      {keysHint && <p className="warning-text">{t("share.keysHint")}</p>}
      <h3>{t("share.own")}</h3>
      {shares && own.length === 0 && <p className="empty">{t("share.noneOwn")}</p>}
      <div className="rows">
        {own.map((share) => (
          <div key={share.area} className="card share-card">
            <span className="card-title">
              {title(share)}
              {share.with_keys && <span className="badge">{t("share.keysBadge")}</span>}
            </span>
            {share.grants.map((g) => (
              <div key={g.account} className="row grant">
                <span className="card-sub">{person(g)}</span>
                <select
                  aria-label={t("share.role")}
                  value={g.role}
                  onChange={(e) => setRole(share, g.account, e.target.value as Role)}
                >
                  <option value="read">{t("share.roleRead")}</option>
                  <option value="write">{t("share.roleWrite")}</option>
                </select>
                <button className="danger" onClick={() => setRole(share, g.account, null)}>
                  {t("share.remove")}
                </button>
              </div>
            ))}
            <div className="buttons">
              <button className="danger" onClick={() => setEnding(share)}>
                {t("share.end")}
              </button>
            </div>
          </div>
        ))}
      </div>
      <h3>{t("share.received")}</h3>
      {shares && received.length === 0 && <p className="empty">{t("share.noneReceived")}</p>}
      <div className="rows">
        {received.map((share) => (
          <div key={share.area} className="card row">
            <span className="card-text">
              <span className="card-title">{title(share) || t("share.untitled")}</span>
              <span className="card-sub">
                {share.role === "write" ? t("share.roleWrite") : t("share.roleRead")}
              </span>
            </span>
          </div>
        ))}
      </div>
      <div className="buttons">
        <button onClick={onClose}>{t("common.close")}</button>
      </div>
      {ending && (
        <ConfirmDialog
          message={t("share.endConfirm", { name: title(ending) })}
          confirmLabel={t("share.end")}
          onConfirm={() => {
            const share = ending;
            setEnding(null);
            void run(unshare(server.id, share.area));
          }}
          onCancel={() => setEnding(null)}
        />
      )}
    </div>
  );
}
