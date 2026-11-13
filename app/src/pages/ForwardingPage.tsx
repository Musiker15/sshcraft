// Port forwarding: every rule of every host, started and stopped with a double click.
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import {
  forwardStart,
  forwardStatuses,
  forwardStop,
  saveHost,
  type ForwardRule,
  type ForwardStatus,
  type Host,
} from "../api";
import { ForwardWizard } from "../components/ForwardWizard";
import { isExposed, sshEquivalent } from "../forms/forwardForm";
import { errorText } from "../i18n/errors";
import { useLanguage, useT } from "../i18n/i18n";
import { ConfirmDialog } from "../shell/ConfirmDialog";
import { ContextMenu, type MenuItem } from "../shell/ContextMenu";
import { Drawer } from "../shell/Drawer";
import { ErrorMessage } from "../shell/ErrorMessage";
import { sortByName } from "./hostsView";

type Props = { hosts: Host[]; onChanged: () => void };
type Menu = { at: { x: number; y: number }; items: MenuItem[] };

/** Statuses are kept per host and rule: a rule id is only unique within its host. */
const keyOf = (hostId: string, ruleId: string) => `${hostId}:${ruleId}`;

/** A rule that is starting, running or reconnecting holds its port. */
function isActive(status: ForwardStatus | undefined): boolean {
  return status !== undefined && status.state !== "stopped" && status.state !== "failed";
}

function dotClass(status: ForwardStatus | undefined): string {
  switch (status?.state) {
    case "running":
      return "dot ok";
    case "starting":
    case "reconnecting":
      return "dot busy";
    case "failed":
      return "dot bad";
    default:
      return "dot";
  }
}

export function ForwardingPage({ hosts, onChanged }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [statuses, setStatuses] = useState<Record<string, ForwardStatus>>({});
  const [wizard, setWizard] = useState<{ editing?: { hostId: string; rule: ForwardRule } } | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [confirm, setConfirm] = useState<{ host: Host; rule: ForwardRule } | null>(null);
  const [problem, setProblem] = useState<unknown>(null);

  useEffect(() => {
    let live = true;
    // Listen first, then fetch: nothing sent in between is lost. An event wins over the
    // snapshot, which may be older.
    const seen = new Set<string>();
    const stop = listen<ForwardStatus>("forward-status", (event) => {
      const key = keyOf(event.payload.host_id, event.payload.rule_id);
      seen.add(key);
      setStatuses((all) => {
        const next = { ...all };
        if (event.payload.state === "stopped") delete next[key];
        else next[key] = event.payload;
        return next;
      });
    });
    void stop.then(() =>
      forwardStatuses().then(
        (list) => {
          if (!live) return;
          setStatuses((all) => {
            const next = { ...all };
            for (const s of list ?? []) {
              const key = keyOf(s.host_id, s.rule_id);
              if (!seen.has(key)) next[key] = s;
            }
            return next;
          });
        },
        () => {},
      ),
    );
    return () => {
      live = false;
      void stop.then((unlisten) => unlisten());
    };
  }, []);

  const statusOf = (hostId: string, ruleId: string) => statuses[keyOf(hostId, ruleId)];

  const toggle = (host: Host, rule: ForwardRule) => {
    const action = isActive(statusOf(host.id, rule.id))
      ? forwardStop(host.id, rule.id)
      : forwardStart(host.id, rule.id);
    void action.catch(setProblem);
  };

  // Rules that still run although their host or rule is gone: they can only be stopped.
  const known = new Set(hosts.flatMap((h) => (h.forwards ?? []).map((r) => keyOf(h.id, r.id))));
  const orphans = Object.entries(statuses)
    .filter(([key, status]) => !known.has(key) && isActive(status))
    .map(([, status]) => status);

  async function remove(host: Host, rule: ForwardRule) {
    try {
      await saveHost({ ...host, forwards: (host.forwards ?? []).filter((r) => r.id !== rule.id) });
      onChanged();
    } catch (e) {
      setProblem(e);
    }
  }

  const withRules = sortByName(hosts, lang).filter((h) => (h.forwards ?? []).length > 0);

  function stateText(status: ForwardStatus | undefined): string {
    switch (status?.state) {
      case "running":
        return t("forwarding.state.running");
      case "starting":
        return t("forwarding.state.starting");
      case "reconnecting":
        return t("forwarding.state.reconnecting", { attempt: status.attempt, seconds: status.delay_secs });
      case "failed":
        return t("forwarding.state.failed");
      default:
        return t("forwarding.state.stopped");
    }
  }

  return (
    <section className="page forwarding">
      <header className="page-head">
        <h1>{t("nav.forwarding")}</h1>
        <div className="page-actions">
          <button className="primary" disabled={hosts.length === 0} onClick={() => setWizard({})}>
            {t("forwarding.new")}
          </button>
        </div>
      </header>
      <p className="hint">{t("forwarding.hint")}</p>
      {problem !== null && <ErrorMessage error={problem} />}
      {withRules.length === 0 && <p className="empty">{t("forwarding.empty")}</p>}
      {withRules.map((host) => (
        <div key={host.id} className="group-block">
          <h2 className="group-title">{host.name}</h2>
          <div className="list">
            {(host.forwards ?? []).map((rule) => {
              const status = statusOf(host.id, rule.id);
              const active = isActive(status);
              return (
                <div
                  key={rule.id}
                  className={active ? "list-row forward-row active" : "list-row forward-row"}
                  tabIndex={0}
                  onDoubleClick={() => toggle(host, rule)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") toggle(host, rule);
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({
                      at: { x: e.clientX, y: e.clientY },
                      items: [
                        { label: active ? t("forwarding.stop") : t("forwarding.start"), onSelect: () => toggle(host, rule) },
                        ...(active
                          ? []
                          : [{ label: t("forwarding.edit"), onSelect: () => setWizard({ editing: { hostId: host.id, rule } }) }]),
                        { label: t("common.delete"), danger: true, onSelect: () => setConfirm({ host, rule }) },
                      ],
                    });
                  }}
                >
                  <span className={dotClass(status)} aria-hidden="true" />
                  <span className="palette-name">{rule.name || t(`forwarding.kind.${rule.kind}`)}</span>
                  <code className="card-sub">{sshEquivalent(rule)}</code>
                  <span className="card-sub">
                    {stateText(status)}
                    {status?.state === "running" && status.connections > 0 && (
                      <> · {t("forwarding.connections", { count: status.connections })}</>
                    )}
                  </span>
                  {isExposed(rule) && <span className="warning-text">{t("forwarding.exposed")}</span>}
                  {status?.state === "running" && status.refused > 0 && (
                    <span className="warning-text forward-error">{t("forwarding.refused", { count: status.refused })}</span>
                  )}
                  {status?.state === "running" && status.notice && (
                    <span className="warning-text forward-error">{t(`errors.${status.notice}`)}</span>
                  )}
                  {status?.state === "failed" && (
                    <span className="error forward-error">{errorText(t, { code: status.code ?? "", detail: status.detail })}</span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}

      {orphans.length > 0 && (
        <div className="group-block">
          <h2 className="group-title">{t("forwarding.orphanGroup")}</h2>
          <div className="list">
            {orphans.map((status) => (
              <div
                key={keyOf(status.host_id, status.rule_id)}
                className="list-row forward-row active"
                tabIndex={0}
                onDoubleClick={() => void forwardStop(status.host_id, status.rule_id).catch(setProblem)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void forwardStop(status.host_id, status.rule_id).catch(setProblem);
                }}
              >
                <span className={dotClass(status)} aria-hidden="true" />
                <span className="palette-name">{t("forwarding.orphan")}</span>
                <span className="card-sub">{stateText(status)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {menu && <ContextMenu items={menu.items} at={menu.at} onClose={() => setMenu(null)} />}

      {wizard && (
        <Drawer onClose={() => setWizard(null)}>
          <ForwardWizard
            hosts={hosts}
            editing={wizard.editing}
            onDone={() => {
              setWizard(null);
              onChanged();
            }}
            onCancel={() => setWizard(null)}
          />
        </Drawer>
      )}

      {confirm && (
        <ConfirmDialog
          message={t("forwarding.deleteConfirm", { name: confirm.rule.name || sshEquivalent(confirm.rule) })}
          confirmLabel={t("common.delete")}
          onConfirm={() => {
            const pending = confirm;
            setConfirm(null);
            if (isActive(statusOf(pending.host.id, pending.rule.id)))
              void forwardStop(pending.host.id, pending.rule.id).catch(() => {});
            void remove(pending.host, pending.rule);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </section>
  );
}
