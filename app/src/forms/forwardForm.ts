// The forwarding rule form: drafts as typed, checked like Rust (sshcraft_core::forward).
import type { ForwardKind, ForwardRule } from "../api";

export type ForwardDraft = {
  kind: ForwardKind;
  name: string;
  bind_address: string;
  bind_port: string;
  target_host: string;
  target_port: string;
};

const MAX_NAME = 100;
const MAX_ADDRESS = 253;
/** The usual SOCKS port, offered for a new dynamic rule. */
const SOCKS_PORT = "1080";

export function emptyDraft(kind: ForwardKind): ForwardDraft {
  return {
    kind,
    name: "",
    // A remote rule listens on the server; "localhost" keeps it to the server itself.
    bind_address: kind === "remote" ? "localhost" : "127.0.0.1",
    bind_port: kind === "dynamic" ? SOCKS_PORT : "",
    target_host: "",
    target_port: "",
  };
}

/** The rule as a draft, for editing. */
export function fromRule(rule: ForwardRule): ForwardDraft {
  return {
    kind: rule.kind,
    name: rule.name,
    bind_address: rule.bind_address,
    bind_port: String(rule.bind_port),
    target_host: rule.target_host,
    target_port: rule.kind === "dynamic" ? "" : String(rule.target_port),
  };
}

function port(text: string): number | null {
  if (!/^\d{1,5}$/.test(text.trim())) return null;
  const value = Number(text.trim());
  return value >= 1 && value <= 65535 ? value : null;
}

function addressOk(text: string): boolean {
  return text !== "" && [...text].length <= MAX_ADDRESS && !/[\s\p{Cc}]/u.test(text);
}

/** The translation key of the first problem, or `null` when the draft is a valid rule. */
export function validateDraft(draft: ForwardDraft): string | null {
  if ([...draft.name.trim()].length > MAX_NAME) return "forwarding.error.name";
  if (!addressOk(draft.bind_address)) return "forwarding.error.bindAddress";
  if (port(draft.bind_port) === null) return "forwarding.error.bindPort";
  if (draft.kind === "dynamic") return null;
  if (!addressOk(draft.target_host.trim())) return "forwarding.error.targetHost";
  if (port(draft.target_port) === null) return "forwarding.error.targetPort";
  return null;
}

/** A valid draft as a rule (check it with `validateDraft` first). */
export function toRule(draft: ForwardDraft, id: string): ForwardRule {
  const dynamic = draft.kind === "dynamic";
  return {
    id,
    name: draft.name.trim(),
    kind: draft.kind,
    bind_address: draft.bind_address,
    bind_port: port(draft.bind_port) ?? 0,
    target_host: dynamic ? "" : draft.target_host.trim(),
    target_port: dynamic ? 0 : (port(draft.target_port) ?? 0),
  };
}

/** An IPv6 address goes in brackets, as ssh writes it. */
function host(address: string): string {
  return address.includes(":") ? `[${address}]` : address;
}

/** The same rule as an option of the `ssh` command. */
export function sshEquivalent(rule: ForwardRule): string {
  const bind = `${host(rule.bind_address)}:${rule.bind_port}`;
  switch (rule.kind) {
    case "local":
      return `-L ${bind}:${host(rule.target_host)}:${rule.target_port}`;
    case "remote":
      return `-R ${bind}:${host(rule.target_host)}:${rule.target_port}`;
    case "dynamic":
      return `-D ${bind}`;
  }
}

/** Whether the rule listens for other machines too (not only loopback). */
export function isExposed(rule: ForwardRule): boolean {
  const address = rule.bind_address.trim().toLowerCase();
  if (address === "localhost" || address === "::1" || address === "[::1]") return false;
  return !/^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(address);
}
