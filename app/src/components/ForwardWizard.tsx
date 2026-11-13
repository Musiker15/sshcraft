// "New forwarding rule": kind, host, addresses and ports, summary. Editing starts at the details.
import { ArrowLeftRight, ArrowRightLeft, Globe } from "lucide-react";
import { useState } from "react";
import { listHosts, saveHost, type ForwardKind, type ForwardRule, type Host } from "../api";
import { emptyDraft, fromRule, isExposed, sshEquivalent, toRule, validateDraft, type ForwardDraft } from "../forms/forwardForm";
import { useLanguage, useT } from "../i18n/i18n";
import { sortByName } from "../pages/hostsView";
import { ErrorMessage } from "../shell/ErrorMessage";

const KINDS: { kind: ForwardKind; icon: typeof Globe }[] = [
  { kind: "local", icon: ArrowRightLeft },
  { kind: "remote", icon: ArrowLeftRight },
  { kind: "dynamic", icon: Globe },
];

type Step = "kind" | "host" | "details" | "summary";

type Props = {
  hosts: Host[];
  /** The rule to change; its host stays. */
  editing?: { hostId: string; rule: ForwardRule };
  onDone: () => void;
  onCancel: () => void;
};

export function ForwardWizard({ hosts, editing, onDone, onCancel }: Props) {
  const t = useT();
  const lang = useLanguage();
  const [step, setStep] = useState<Step>(editing ? "details" : "kind");
  const [draft, setDraft] = useState<ForwardDraft>(editing ? fromRule(editing.rule) : emptyDraft("local"));
  const [hostId, setHostId] = useState(editing?.hostId ?? "");
  const [problem, setProblem] = useState<unknown>(null);
  const error = validateDraft(draft);
  const rule = toRule(draft, editing?.rule.id ?? crypto.randomUUID());
  const set = (patch: Partial<ForwardDraft>) => setDraft((d) => ({ ...d, ...patch }));

  async function save() {
    try {
      // The host as stored now: a change from sync since the page loaded must not be undone.
      const stored = (await listHosts()) ?? hosts;
      const host = stored.find((h) => h.id === hostId);
      const rules = host?.forwards ?? [];
      if (!host || (editing && !rules.some((r) => r.id === rule.id))) {
        setProblem({ code: "not_found", detail: null });
        return;
      }
      const forwards = editing ? rules.map((r) => (r.id === rule.id ? rule : r)) : [...rules, rule];
      await saveHost({ ...host, forwards });
      onDone();
    } catch (e) {
      setProblem(e);
    }
  }

  const field = (label: string, value: string, change: (v: string) => void, inputMode?: "numeric") => (
    <label>
      {label}
      <input value={value} inputMode={inputMode} onChange={(e) => change(e.target.value)} />
    </label>
  );

  return (
    <div className="dialog forward-wizard">
      <h2>{editing ? t("forwarding.editTitle") : t("forwarding.newTitle")}</h2>

      {step === "kind" && (
        <div className="kind-choice">
          {KINDS.map(({ kind, icon: Icon }) => (
            <button
              key={kind}
              className={draft.kind === kind ? "kind-card active" : "kind-card"}
              aria-pressed={draft.kind === kind}
              onClick={() => setDraft({ ...emptyDraft(kind), name: draft.name })}
            >
              <Icon size={18} aria-hidden="true" />
              <strong>{t(`forwarding.kind.${kind}`)}</strong>
              <span className="hint">{t(`forwarding.kindHint.${kind}`)}</span>
            </button>
          ))}
        </div>
      )}

      {step === "host" && (
        <label>
          {t("forwarding.host")}
          <select value={hostId} onChange={(e) => setHostId(e.target.value)}>
            <option value="">{t("forwarding.pickHost")}</option>
            {sortByName(hosts, lang).map((h) => (
              <option key={h.id} value={h.id}>
                {h.name}
              </option>
            ))}
          </select>
        </label>
      )}

      {step === "details" && (
        <>
          <p className="hint">{t(`forwarding.kindHint.${draft.kind}`)}</p>
          {field(t("forwarding.name"), draft.name, (name) => set({ name }))}
          <div className="grid-2">
            {field(t("forwarding.bindAddress"), draft.bind_address, (bind_address) => set({ bind_address }))}
            {field(t("forwarding.bindPort"), draft.bind_port, (bind_port) => set({ bind_port }), "numeric")}
          </div>
          {draft.kind !== "dynamic" && (
            <div className="grid-2">
              {field(t("forwarding.targetHost"), draft.target_host, (target_host) => set({ target_host }))}
              {field(t("forwarding.targetPort"), draft.target_port, (target_port) => set({ target_port }), "numeric")}
            </div>
          )}
          {!error && isExposed(rule) && <p className="warning-text">{t("forwarding.exposedWarning")}</p>}
          {error && <p className="error">{t(error)}</p>}
        </>
      )}

      {step === "summary" && (
        <div className="forward-summary">
          <p>{t(`forwarding.summary.${draft.kind}`, summaryValues(rule))}</p>
          <p className="hint">{t("forwarding.sshEquivalent")}</p>
          <code>{sshEquivalent(rule)}</code>
          {isExposed(rule) && <p className="warning-text">{t("forwarding.exposedWarning")}</p>}
        </div>
      )}

      {problem !== null && <ErrorMessage error={problem} />}

      <div className="buttons">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        {step !== "kind" && !(editing && step === "details") && (
          <button onClick={() => setStep(step === "summary" ? "details" : step === "details" ? "host" : "kind")}>
            {t("forwarding.back")}
          </button>
        )}
        {step !== "summary" ? (
          <button
            className="primary"
            disabled={(step === "host" && hostId === "") || (step === "details" && error !== null)}
            onClick={() => setStep(step === "kind" ? "host" : step === "host" ? "details" : "summary")}
          >
            {t("forwarding.next")}
          </button>
        ) : (
          <button className="primary" onClick={() => void save()}>
            {t("common.save")}
          </button>
        )}
      </div>
    </div>
  );
}

function summaryValues(rule: ForwardRule): Record<string, string> {
  return {
    bind: `${rule.bind_address}:${rule.bind_port}`,
    target: `${rule.target_host}:${rule.target_port}`,
  };
}
