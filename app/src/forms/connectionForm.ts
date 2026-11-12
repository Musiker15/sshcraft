// The connection options of a host or group form: identity, jump hosts, proxy, agent forwarding
// and environment variables. Address, port, user and key are in hostForm.ts.
import type { ConnectionSettings, EnvVar, Proxy } from "../api";
import { FormError } from "../i18n/errors";
import { parsePort } from "./hostForm";

export type JumpMode = "inherit" | "none" | "list";
export type ProxyMode = "inherit" | "direct" | "http" | "socks5";
export type AgentMode = "inherit" | "on" | "off";

export type ConnectionFormValues = {
  /** "" inherits. */
  identity: string;
  jumpMode: JumpMode;
  jumps: string[];
  proxyMode: ProxyMode;
  proxyAddress: string;
  proxyPort: string;
  proxyUser: string;
  agent: AgentMode;
  env: EnvVar[];
};

export type ConnectionOptions = Pick<
  ConnectionSettings,
  "identity" | "jump_hosts" | "proxy" | "agent_forwarding" | "env"
>;

/** Names the core accepts: a letter or underscore, then letters, digits, underscores; at most 64. */
export const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;
const MAX_ENV_VALUE = 4096;

export function validEnvVar(v: EnvVar): boolean {
  return ENV_NAME.test(v.name.trim()) && new TextEncoder().encode(v.value).length <= MAX_ENV_VALUE;
}

export const emptyConnection: ConnectionFormValues = {
  identity: "",
  jumpMode: "inherit",
  jumps: [],
  proxyMode: "inherit",
  proxyAddress: "",
  proxyPort: "",
  proxyUser: "",
  agent: "inherit",
  env: [],
};

function proxy(form: ConnectionFormValues): Proxy | null {
  switch (form.proxyMode) {
    case "inherit":
      return null;
    case "direct":
      return { kind: "direct" };
    default: {
      const address = form.proxyAddress.trim();
      if (address === "") throw new FormError("proxy_address_required");
      const port = parsePort(form.proxyPort);
      if (port === null) throw new FormError("proxy_port_required");
      const user = form.proxyUser.trim();
      return { kind: form.proxyMode, address, port, username: user === "" ? null : user };
    }
  }
}

export function toConnectionOptions(form: ConnectionFormValues): ConnectionOptions {
  return {
    identity: form.identity === "" ? null : form.identity,
    jump_hosts: form.jumpMode === "inherit" ? null : form.jumpMode === "none" ? [] : form.jumps,
    proxy: proxy(form),
    agent_forwarding: form.agent === "inherit" ? null : form.agent === "on",
    // Rows with an invalid name are not saved (the form marks them).
    env: form.env.filter(validEnvVar).map((v) => ({ name: v.name.trim(), value: v.value })),
  };
}

export function fromConnectionOptions(settings: ConnectionSettings): ConnectionFormValues {
  const p = settings.proxy ?? null;
  const jumps = settings.jump_hosts ?? null;
  return {
    identity: settings.identity ?? "",
    jumpMode: jumps === null ? "inherit" : jumps.length === 0 ? "none" : "list",
    jumps: jumps ?? [],
    proxyMode: p === null ? "inherit" : p.kind,
    proxyAddress: p && p.kind !== "direct" ? p.address : "",
    proxyPort: p && p.kind !== "direct" ? String(p.port) : "",
    proxyUser: p && p.kind !== "direct" ? (p.username ?? "") : "",
    agent: settings.agent_forwarding == null ? "inherit" : settings.agent_forwarding ? "on" : "off",
    env: settings.env ?? [],
  };
}
