// Typed wrappers around the Tauri commands. Field names mirror the Rust serde output.
import { Channel, invoke } from "@tauri-apps/api/core";

export type KeyRef = { kind: "stored"; id: string } | { kind: "agent"; fingerprint: string };

export type AgentKey = { fingerprint: string; algorithm: string; comment: string; public_key: string };

export type ConnectionSettings = {
  address: string | null;
  port: number | null;
  username: string | null;
  key: KeyRef | null;
};

export type Group = { id: string; name: string; defaults: ConnectionSettings };

export type Host = {
  id: string;
  name: string;
  group_id: string | null;
  settings: ConnectionSettings;
};

export type KeyInfo = {
  id: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  public_key: string;
};

export type EndReason =
  | "exited"
  | "lost"
  | "host_key_rejected"
  | "host_key_mismatch"
  | "auth_failed"
  | "error";

export type SessionStatus =
  | { state: "connecting" }
  | { state: "connected" }
  | { state: "reconnecting"; attempt: number; delay_secs: number }
  | { state: "ended"; reason: EndReason; detail: string | null };

export type HostKeyPrompt = {
  prompt_id: number;
  host_name: string;
  address: string;
  port: number;
  fingerprint: string;
};

export type CommandError = { code: string; message: string };

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as CommandError).message);
  return String(e);
}

export const listGroups = () => invoke<Group[]>("list_groups");
export const saveGroup = (group: Group) => invoke<void>("save_group", { group });
export const deleteGroup = (id: string) => invoke<void>("delete_group", { id });
export const listHosts = () => invoke<Host[]>("list_hosts");
export const saveHost = (host: Host) => invoke<void>("save_host", { host });
export const deleteHost = (id: string) => invoke<void>("delete_host", { id });
export const forgetHostKey = (hostId: string) => invoke<void>("forget_host_key", { hostId });

export const vaultStatus = () => invoke<{ ok: boolean; error: string | null }>("vault_status");
export const listKeys = () => invoke<KeyInfo[]>("list_keys");
export const importKey = (name: string, text: string, passphrase: string | null) =>
  invoke<KeyInfo>("import_key", { name, text, passphrase });
export const generateKey = (name: string) => invoke<KeyInfo>("generate_key", { name });
export const deleteKey = (id: string) => invoke<void>("delete_key", { id });
export const listAgentKeys = () => invoke<AgentKey[]>("list_agent_keys");

export type ImportCandidate = {
  alias: string;
  group: string | null;
  settings: ConnectionSettings;
  key_note: string | null;
};
export type SshConfigPreview = { candidates: ImportCandidate[]; skipped: string[] };
export const sshConfigPreview = () => invoke<SshConfigPreview>("ssh_config_preview");
export const sshConfigImport = (aliases: string[]) => invoke<{ created: number }>("ssh_config_import", { aliases });

export type PassCliStatus = "not_installed" | "logged_out" | "ready";
export type PassKey = { share_id: string; item_id: string; title: string };
export const passCliStatus = () => invoke<PassCliStatus>("pass_cli_status");
export const passCliListKeys = () => invoke<PassKey[]>("pass_cli_list_keys");
export const importFromPassCli = (shareId: string, itemId: string, name: string) =>
  invoke<KeyInfo>("import_from_pass_cli", { shareId, itemId, name });

export async function sessionOpen(
  hostId: string,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onStatus: (status: SessionStatus) => void,
): Promise<string> {
  const data = new Channel<ArrayBuffer | number[]>();
  data.onmessage = (msg) => onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
  const status = new Channel<SessionStatus>();
  status.onmessage = onStatus;
  return invoke<string>("session_open", { hostId, cols, rows, onData: data, onStatus: status });
}

const encoder = new TextEncoder();
export const sessionWrite = (id: string, text: string) =>
  invoke<void>("session_write", { id, data: Array.from(encoder.encode(text)) });
export const sessionResize = (id: string, cols: number, rows: number) =>
  invoke<void>("session_resize", { id, cols, rows });
export const sessionAck = (id: string, bytes: number) => invoke<void>("session_ack", { id, bytes });
export const sessionClose = (id: string) => invoke<void>("session_close", { id });
export const answerHostKey = (promptId: number, accept: boolean) =>
  invoke<void>("answer_host_key", { promptId, accept });
