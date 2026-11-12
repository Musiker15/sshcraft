import { useState } from "react";
import {
  confirmRecipient,
  findRecipient,
  shareItems,
  type Group,
  type Host,
  type Recipient,
  type Role,
  type ServerView,
  type ShareWhatInput,
} from "../../api";
import { useT } from "../../i18n/i18n";
import { ErrorMessage } from "../../shell/ErrorMessage";
import { SafetyNumber } from "./SafetyNumber";

export type ShareTarget = { kind: "group"; group: Group } | { kind: "host"; host: Host };

type Props = {
  server: ServerView;
  target: ShareTarget;
  /** Every host, to offer the group's hosts for a selection. */
  hosts: Host[];
  onDone: () => void;
  onCancel: () => void;
};

/** Shares a group or host with someone on the same server, after comparing safety numbers. */
export function ShareDialog({ server, target, hosts, onDone, onCancel }: Props) {
  const t = useT();
  const [email, setEmail] = useState("");
  const [recipient, setRecipient] = useState<Recipient | null>(null);
  const [compared, setCompared] = useState(false);
  const [acceptChanged, setAcceptChanged] = useState(false);
  const [role, setRole] = useState<Role>("read");
  const [withKeys, setWithKeys] = useState(false);
  const [allHosts, setAllHosts] = useState(true);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  const groupHosts = target.kind === "group" ? hosts.filter((h) => h.group_id === target.group.id) : [];
  const shared = target.kind === "host" ? [target.host] : allHosts ? groupHosts : groupHosts.filter((h) => picked.has(h.id));
  const agentKeys =
    shared.filter((h) => h.settings.key?.kind === "agent").length +
    (target.kind === "group" && target.group.defaults.key?.kind === "agent" ? 1 : 0);
  const known = recipient?.known.state;
  const trusted = known === "confirmed" || (known === "changed" ? acceptChanged : compared);
  const ready = recipient !== null && trusted && (target.kind === "host" || allHosts || picked.size > 0) && !busy;

  async function find() {
    setError(null);
    setRecipient(null);
    setCompared(false);
    setAcceptChanged(false);
    try {
      setRecipient(await findRecipient(server.id, email));
    } catch (e) {
      setError(e);
    }
  }

  async function share() {
    if (!recipient) return;
    const what: ShareWhatInput =
      target.kind === "host"
        ? { kind: "host", host: target.host.id }
        : allHosts
          ? { kind: "group_all", group: target.group.id }
          : { kind: "selection", group: target.group.id, hosts: groupHosts.filter((h) => picked.has(h.id)).map((h) => h.id) };
    setBusy(true);
    setError(null);
    try {
      if (known !== "confirmed") await confirmRecipient(server.id, recipient.email, recipient.account);
      await shareItems(server.id, what, [{ account: recipient.account, role }], withKeys);
      onDone();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  const toggle = (id: string) =>
    setPicked((before) => {
      const next = new Set(before);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const name = target.kind === "group" ? target.group.name : target.host.name;
  return (
    <div className="dialog share-dialog">
      <h2>{t("share.title", { name })}</h2>
      <p className="hint">{t("share.intro", { server: server.name })}</p>
      <form
        className="row-form"
        onSubmit={(e) => {
          e.preventDefault();
          void find();
        }}
      >
        <label>
          {t("share.email")}
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        </label>
        <button type="submit" disabled={email.trim() === ""}>
          {t("share.find")}
        </button>
      </form>
      {error !== null && <ErrorMessage error={error} />}

      {recipient && (
        <>
          <div className="recipient">
            <span className="card-title">{recipient.name}</span>
            <span className="card-sub">{recipient.email}</span>
          </div>
          {known === "changed" && recipient.known.state === "changed" ? (
            <div className="warning-box">
              <p>{t("share.changedWarning", { name: recipient.name })}</p>
              <p className="hint">{t("share.previousNumber")}</p>
              <SafetyNumber value={recipient.known.previous_safety_number} />
              <p className="hint">{t("share.newNumber")}</p>
              <SafetyNumber value={recipient.safety_number} />
              <label className="check">
                <input type="checkbox" checked={acceptChanged} onChange={(e) => setAcceptChanged(e.target.checked)} />
                {t("share.acceptChanged")}
              </label>
            </div>
          ) : (
            <>
              <SafetyNumber value={recipient.safety_number} />
              <p className="hint">{t("share.compareHint", { name: recipient.name })}</p>
              {known !== "confirmed" && (
                <label className="check">
                  <input type="checkbox" checked={compared} onChange={(e) => setCompared(e.target.checked)} />
                  {t("share.compared")}
                </label>
              )}
            </>
          )}

          <label>
            {t("share.role")}
            <select value={role} onChange={(e) => setRole(e.target.value as Role)}>
              <option value="read">{t("share.roleRead")}</option>
              <option value="write">{t("share.roleWrite")}</option>
            </select>
          </label>

          {target.kind === "group" && (
            <fieldset className="choices">
              <label className="check">
                <input type="radio" name="scope" checked={allHosts} onChange={() => setAllHosts(true)} />
                {t("share.allHosts")}
              </label>
              <label className="check">
                <input type="radio" name="scope" checked={!allHosts} onChange={() => setAllHosts(false)} />
                {t("share.selection")}
              </label>
              {!allHosts &&
                groupHosts.map((h) => (
                  <label key={h.id} className="check indent">
                    <input type="checkbox" checked={picked.has(h.id)} onChange={() => toggle(h.id)} />
                    {h.name}
                  </label>
                ))}
            </fieldset>
          )}

          <label className="check">
            <input type="checkbox" checked={withKeys} onChange={(e) => setWithKeys(e.target.checked)} />
            {t("share.withKeys")}
          </label>
          {withKeys && agentKeys > 0 && <p className="hint">{t("share.agentKeys", { count: agentKeys })}</p>}
        </>
      )}

      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button className="primary" disabled={!ready} onClick={() => void share()}>
          {t("share.share")}
        </button>
      </div>
    </div>
  );
}
