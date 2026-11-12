// Typed wrappers around the Tauri commands. Field names mirror the Rust serde output.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { CommandError } from "./i18n/errors";

export type KeyRef = { kind: "stored"; id: string } | { kind: "agent"; fingerprint: string };

export type AgentKey = { fingerprint: string; algorithm: string; comment: string; public_key: string };

export type ConnectionSettings = {
  address: string | null;
  port: number | null;
  username: string | null;
  key: KeyRef | null;
};

export const HOST_OS = [
  "linux",
  "debian",
  "ubuntu",
  "fedora",
  "red_hat",
  "cent_os",
  "rocky",
  "alma",
  "arch",
  "alpine",
  "open_suse",
  "raspbian",
  "free_bsd",
  "open_bsd",
  "mac_os",
  "windows",
  "other",
] as const;
export type HostOs = (typeof HOST_OS)[number];

/** `location`: the sync server an item belongs to; missing or null means "only on this device". */
export type Group = {
  id: string;
  name: string;
  defaults: ConnectionSettings;
  location?: string | null;
  parent_id?: string | null;
  tags?: string[];
};

export type Host = {
  id: string;
  name: string;
  group_id: string | null;
  settings: ConnectionSettings;
  location?: string | null;
  tags?: string[];
  os?: HostOs | null;
};

export type KeyInfo = {
  id: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  public_key: string;
  location?: string | null;
};

export type EndReason =
  | "exited"
  | "lost"
  | "host_key_rejected"
  | "host_key_mismatch"
  | "auth_failed"
  | "connect_failed"
  | "error";

export type SessionStatus =
  | { state: "connecting" }
  | { state: "connected" }
  | { state: "reconnecting"; attempt: number; delay_secs: number }
  | { state: "ended"; reason: EndReason; code: string | null; detail: string | null };

export type HostKeyPrompt = {
  prompt_id: number;
  host_name: string;
  address: string;
  port: number;
  fingerprint: string;
};

export type { CommandError } from "./i18n/errors";

export const listGroups = () => invoke<Group[]>("list_groups");
export const saveGroup = (group: Group) => invoke<void>("save_group", { group });
export const deleteGroup = (id: string) => invoke<void>("delete_group", { id });
export const listHosts = () => invoke<Host[]>("list_hosts");
export const saveHost = (host: Host) => invoke<void>("save_host", { host });
export const deleteHost = (id: string) => invoke<void>("delete_host", { id });
export const forgetHostKey = (hostId: string) => invoke<void>("forget_host_key", { hostId });

export const helloStatus = () => invoke<{ missing: boolean }>("hello_status");
export type VaultStatus = { ok: boolean; code: string | null; detail: string | null };
export const vaultStatus = () => invoke<VaultStatus>("vault_status");
export const listKeys = () => invoke<KeyInfo[]>("list_keys");
export const importKey = (name: string, text: string, passphrase: string | null) =>
  invoke<KeyInfo>("import_key", { name, text, passphrase });
export const generateKey = (name: string) => invoke<KeyInfo>("generate_key", { name });
export const deleteKey = (id: string) => invoke<void>("delete_key", { id });
export const listAgentKeys = () => invoke<AgentKey[]>("list_agent_keys");

export type PassCliStatus = "not_installed" | "logged_out" | "ready" | "failed";
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

export type AppTheme = "system" | "dark" | "light";
export type HostView = "grid" | "list";
export type AppSettings = {
  language: "auto" | "en" | "de";
  app_theme: AppTheme;
  terminal_theme: string;
  font_family: string;
  font_size: number;
  host_view: HostView;
};
export const getSettings = () => invoke<AppSettings>("get_settings");
/** Returns what Rust stored, with values kept in their allowed ranges. */
export const saveSettings = (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings });
export const systemLocale = () => invoke<string | null>("system_locale");
export const listRecent = (limit: number) => invoke<string[]>("list_recent", { limit });

export type SyncStatusView =
  | { state: "idle" }
  | { state: "syncing" }
  | { state: "offline"; retry_in_secs: number }
  | { state: "revoked" }
  | { state: "blocked" }
  | { state: "error"; detail: string }
  | { state: "off" };

export type ServerView = {
  id: string;
  name: string;
  origin: string;
  sync_enabled: boolean;
  sync_keys: boolean;
  revoked: boolean;
  status: SyncStatusView;
};

export type CreateAccountInput = {
  server_name: string;
  origin: string;
  api_key: string;
  name: string;
  email: string;
  device_name: string;
};

export type JoinAccountInput = {
  server_name: string;
  origin: string;
  api_key: string;
  account_key: string;
  device_name: string;
};

export type AccountCreated = {
  /** null: the account exists, but setting it up here did not finish; join with the key. */
  server_id: string | null;
  account_key: string;
  qr_svg: string;
  unfinished: CommandError | null;
};

export type SyncDevice = {
  device_id: string;
  name: string;
  platform: string;
  active: boolean;
  admin_blocked: boolean;
  last_seen: number | null;
  this_device: boolean;
  recovery: boolean;
};

export type ItemKind = "group" | "host" | "key";

export const listServers = () => invoke<ServerView[]>("list_servers");
export const createSyncAccount = (input: CreateAccountInput) =>
  invoke<AccountCreated>("create_sync_account", { input });
export const joinSyncAccount = (input: JoinAccountInput) => invoke<string>("join_sync_account", { input });
export const setServerSync = (id: string, syncEnabled: boolean, syncKeys: boolean) =>
  invoke<void>("set_server_sync", { id, syncEnabled, syncKeys });
export const removeServer = (id: string) => invoke<void>("remove_server", { id });
export const listSyncDevices = (id: string) => invoke<SyncDevice[]>("list_sync_devices", { id });
export const revokeSyncDevice = (id: string, deviceId: string) =>
  invoke<void>("revoke_sync_device", { id, deviceId });
export const moveItem = (kind: ItemKind, itemId: string, location: string | null) =>
  invoke<void>("move_item", { kind, itemId, location });
export const accountKeyQr = (text: string) => invoke<string>("account_key_qr", { text });

export type KnownState =
  | { state: "new" }
  | { state: "confirmed" }
  | { state: "unconfirmed" }
  | { state: "changed"; previous_safety_number: string };
export type Recipient = { account: string; name: string; email: string; safety_number: string; known: KnownState };
export type Role = "read" | "write";
export type ShareKind = "group_all" | "selection" | "host";
export type ShareWhatInput =
  | { kind: "group_all"; group: string }
  | { kind: "selection"; group: string; hosts: string[] }
  | { kind: "host"; host: string };
export type GrantInput = { account: string; role: Role };
export type ShareOutcome = { area: string; agent_keys_skipped: number };
export type RevokeOutcome = { keys_were_shared: boolean };
export type GrantView = { account: string; role: Role; email: string | null; name: string | null };
export type ShareView = {
  area: string;
  kind: ShareKind;
  group: string | null;
  hosts: string[];
  grants: GrantView[];
  with_keys: boolean;
  owned: boolean;
  owner: string;
  role: Role | null;
};
export type ItemShare = { kind: ItemKind; id: string; owned: boolean; owner_name: string | null; role: Role | null };

export const findRecipient = (server: string, email: string) => invoke<Recipient>("find_recipient", { server, email });
export const confirmRecipient = (server: string, email: string, account: string) =>
  invoke<void>("confirm_recipient", { server, email, account });
export const shareItems = (server: string, what: ShareWhatInput, grants: GrantInput[], withKeys: boolean) =>
  invoke<ShareOutcome>("share_items", { server, what, grants, withKeys });
export const listShares = (server: string) => invoke<ShareView[]>("list_shares", { server });
export const setShareGrants = (server: string, area: string, grants: GrantInput[]) =>
  invoke<RevokeOutcome>("set_share_grants", { server, area, grants });
export const unshare = (server: string, area: string) => invoke<RevokeOutcome>("unshare", { server, area });
export const itemShares = () => invoke<ItemShare[]>("item_shares");

export type KeyNote =
  | { kind: "no_identity_file" }
  | { kind: "not_in_agent" }
  | { kind: "agent_unavailable" }
  | { kind: "file"; path: string };
export type ImportWarning =
  | { kind: "proxy_ignored"; directive: string }
  | { kind: "unsupported_token"; token: string }
  | { kind: "password_not_imported" }
  | { kind: "bad_port" }
  | { kind: "unsupported"; what: string };
export type ImportCandidate = {
  name: string;
  group_path: string[];
  group_defaults?: ConnectionSettings;
  settings: ConnectionSettings;
  tags?: string[];
  os?: HostOs | null;
  key_note: KeyNote | null;
  warnings: ImportWarning[];
};
export type ImportPlan = { candidates: ImportCandidate[]; skipped: string[] };
export type ImportPreview = { plan: ImportPlan; host_keys: number | null };
export type ImportOutcome = { created: number; duplicates: number; groups: string[]; hosts: string[] };
export type ImportSource =
  | { kind: "ssh_config" }
  | { kind: "known_hosts"; path: string }
  | { kind: "csv"; path: string }
  | { kind: "putty_registry" }
  | { kind: "putty_reg"; path: string }
  | { kind: "moba_xterm"; path: string }
  | { kind: "secure_crt"; dir: string };
export type ImportDefaults = {
  known_hosts: string | null;
  ssh_dir: string | null;
  mobaxterm: string | null;
  securecrt: string | null;
  putty_registry: boolean;
};
export type KeyFile = {
  path: string;
  name: string;
  format: "open_ssh" | "pem" | "ppk";
  encrypted: boolean;
  algorithm: string | null;
  fingerprint: string | null;
};
export type KnownOutcome = { added: number; same: number; conflicts: [string, number][] };
export type ExportPlace = { kind: "local" } | { kind: "server"; id: string };
export type ExportFilter = { location?: ExportPlace; group?: string };

export const importDefaults = () => invoke<ImportDefaults>("import_defaults");
export const importPreview = (source: ImportSource) => invoke<ImportPreview>("import_preview", { source });
export const importApply = (candidates: ImportCandidate[], location: string | null) =>
  invoke<ImportOutcome>("import_apply", { candidates, location });
export const importKnownHosts = (path: string) => invoke<KnownOutcome>("import_known_hosts", { path });
export const scanKeyFiles = (dir: string) => invoke<KeyFile[]>("scan_key_files", { dir });
export const importKeyFile = (path: string, passphrase: string | null) =>
  invoke<KeyInfo>("import_key_file", { path, passphrase });
export const exportHosts = (format: "csv" | "json", filter: ExportFilter, path: string) =>
  invoke<void>("export_hosts", { format, filter, path });
