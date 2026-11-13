import type { Host, Identity, PasswordChange } from "../api";
import {
  validEnvVar,
  type AgentMode,
  type ConnectionFormValues,
  type JumpMode,
  type ProxyMode,
} from "../forms/connectionForm";
import { useT } from "../i18n/i18n";
import { PasswordField } from "./PasswordField";

type Props = {
  values: ConnectionFormValues;
  onChange: (values: ConnectionFormValues) => void;
  identities: Identity[];
  /** Hosts that can be jump hosts (the edited host is left out). */
  hosts: Host[];
  password: PasswordChange;
  onPassword: (change: PasswordChange) => void;
  passwordStored: boolean;
  proxyPassword: PasswordChange;
  onProxyPassword: (change: PasswordChange) => void;
  proxyPasswordStored: boolean;
};

/** Identity and password, then (folded) jump hosts, proxy, agent forwarding and environment. */
export function ConnectionFields(props: Props) {
  const t = useT();
  const { values, onChange, identities, hosts } = props;
  const set = (change: Partial<ConnectionFormValues>) => onChange({ ...values, ...change });
  const hostName = (id: string) => hosts.find((h) => h.id === id)?.name ?? t("settingsFields.notFound");
  const free = hosts.filter((h) => !values.jumps.includes(h.id));
  const move = (index: number, by: number) => {
    const jumps = [...values.jumps];
    const [item] = jumps.splice(index, 1);
    jumps.splice(index + by, 0, item);
    set({ jumps });
  };
  const hasProxy = values.proxyMode === "http" || values.proxyMode === "socks5";

  return (
    <>
      <label>
        {t("connection.identity")}
        <select value={values.identity} onChange={(e) => set({ identity: e.target.value })}>
          <option value="">{t("connection.identityNone")}</option>
          {identities.map((i) => (
            <option key={i.id} value={i.id}>
              {i.name}
            </option>
          ))}
        </select>
      </label>
      <PasswordField
        label={t("password.label")}
        stored={props.passwordStored}
        value={props.password}
        onChange={props.onPassword}
      />
      <details className="connection-more">
        <summary>{t("connection.advanced")}</summary>

        <label>
          {t("connection.jumps")}
          <select value={values.jumpMode} onChange={(e) => set({ jumpMode: e.target.value as JumpMode })}>
            <option value="inherit">{t("connection.jumpsInherit")}</option>
            <option value="none">{t("connection.jumpsNone")}</option>
            <option value="list">{t("connection.jumpsList")}</option>
          </select>
        </label>
        {values.jumpMode === "list" && (
          <div className="jump-list">
            {values.jumps.map((id, index) => (
              <div key={id} className="jump-row">
                <span>
                  {index + 1}. {hostName(id)}
                </span>
                <button disabled={index === 0} onClick={() => move(index, -1)} aria-label={t("connection.up")}>
                  ↑
                </button>
                <button
                  disabled={index === values.jumps.length - 1}
                  onClick={() => move(index, 1)}
                  aria-label={t("connection.down")}
                >
                  ↓
                </button>
                <button onClick={() => set({ jumps: values.jumps.filter((j) => j !== id) })}>
                  {t("connection.removeJump")}
                </button>
              </div>
            ))}
            {values.jumps.length < 8 && (
              <select
                value=""
                aria-label={t("connection.jumpAdd")}
                onChange={(e) => e.target.value && set({ jumps: [...values.jumps, e.target.value] })}
              >
                <option value="">{t("connection.jumpAdd")}</option>
                {free.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.name}
                  </option>
                ))}
              </select>
            )}
          </div>
        )}

        <label>
          {t("connection.proxy")}
          <select value={values.proxyMode} onChange={(e) => set({ proxyMode: e.target.value as ProxyMode })}>
            <option value="inherit">{t("connection.proxyInherit")}</option>
            <option value="direct">{t("connection.proxyDirect")}</option>
            <option value="http">{t("connection.proxyHttp")}</option>
            <option value="socks5">{t("connection.proxySocks5")}</option>
          </select>
        </label>
        {hasProxy && (
          <>
            <div className="grid-2">
              <label>
                {t("connection.proxyAddress")}
                <input value={values.proxyAddress} onChange={(e) => set({ proxyAddress: e.target.value })} />
              </label>
              <label>
                {t("settingsFields.port")}
                <input
                  value={values.proxyPort}
                  inputMode="numeric"
                  placeholder={values.proxyMode === "http" ? "3128" : "1080"}
                  onChange={(e) => set({ proxyPort: e.target.value })}
                />
              </label>
            </div>
            <label>
              {t("connection.proxyUser")}
              <input value={values.proxyUser} onChange={(e) => set({ proxyUser: e.target.value })} />
            </label>
            <PasswordField
              label={t("connection.proxyPassword")}
              stored={props.proxyPasswordStored}
              value={props.proxyPassword}
              onChange={props.onProxyPassword}
            />
            {values.proxyMode === "http" && <p className="hint">{t("connection.proxyHttpHint")}</p>}
          </>
        )}

        <label>
          {t("connection.agent")}
          <select value={values.agent} onChange={(e) => set({ agent: e.target.value as AgentMode })}>
            <option value="inherit">{t("connection.agentInherit")}</option>
            <option value="on">{t("connection.agentOn")}</option>
            <option value="off">{t("connection.agentOff")}</option>
          </select>
        </label>
        {values.agent === "on" && <p className="warning-text">{t("connection.agentWarning")}</p>}

        <label>
          {t("connection.record")}
          <select value={values.record} onChange={(e) => set({ record: e.target.value as AgentMode })}>
            <option value="inherit">{t("connection.recordInherit")}</option>
            <option value="on">{t("connection.recordOn")}</option>
            <option value="off">{t("connection.recordOff")}</option>
          </select>
        </label>

        <div className="env-list">
          <span className="field-label">{t("connection.env")}</span>
          {values.env.map((v, index) => (
            <div key={index} className="env-row">
              <input
                value={v.name}
                aria-label={t("connection.envName")}
                placeholder={t("connection.envName")}
                aria-invalid={v.name !== "" && !validEnvVar(v)}
                onChange={(e) =>
                  set({ env: values.env.map((x, i) => (i === index ? { ...x, name: e.target.value } : x)) })
                }
              />
              <input
                value={v.value}
                aria-label={t("connection.envValue")}
                placeholder={t("connection.envValue")}
                onChange={(e) =>
                  set({ env: values.env.map((x, i) => (i === index ? { ...x, value: e.target.value } : x)) })
                }
              />
              <button onClick={() => set({ env: values.env.filter((_, i) => i !== index) })}>
                {t("connection.removeJump")}
              </button>
              {v.name !== "" && !validEnvVar(v) && <span className="error">{t("connection.envInvalid")}</span>}
            </div>
          ))}
          <div className="buttons">
            <button onClick={() => set({ env: [...values.env, { name: "", value: "" }] })}>
              {t("connection.envAdd")}
            </button>
          </div>
          <p className="hint">{t("connection.envHint")}</p>
        </div>
      </details>
    </>
  );
}
