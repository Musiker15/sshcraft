// Typed wrappers around the Tauri commands. Field names mirror the Rust serde output.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { CommandError } from "./i18n/errors";

export type KeyRef = { kind: "stored"; id: string } | { kind: "agent"; fingerprint: string };

export type AgentKey = { fingerprint: string; algorithm: string; comment: string; public_key: string };

export type ProxyEndpoint = { address: string; port: number; username: string | null; password?: null };
export type Proxy = { kind: "direct" } | ({ kind: "http" } & ProxyEndpoint) | ({ kind: "socks5" } & ProxyEndpoint);
export type EnvVar = { name: string; value: string };

/** Password ids never reach the UI: a view only says whether one is set. */
export type ConnectionSettings = {
  address: string | null;
  port: number | null;
  username: string | null;
  key: KeyRef | null;
  identity?: string | null;
  /** null: inherit; []: none. */
  jump_hosts?: string[] | null;
  proxy?: Proxy | null;
  agent_forwarding?: boolean | null;
  env?: EnvVar[];
  /** Record sessions; null or missing: as the group above, then the app setting. */
  session_log?: boolean | null;
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
  has_password?: boolean;
  proxy_has_password?: boolean;
};

export type ForwardKind = "local" | "remote" | "dynamic";
export type ForwardRule = {
  id: string;
  name: string;
  kind: ForwardKind;
  bind_address: string;
  bind_port: number;
  /** Empty for a dynamic rule. */
  target_host: string;
  /** 0 for a dynamic rule. */
  target_port: number;
};
export type Host = {
  id: string;
  name: string;
  group_id: string | null;
  settings: ConnectionSettings;
  location?: string | null;
  tags?: string[];
  os?: HostOs | null;
  has_password?: boolean;
  proxy_has_password?: boolean;
  /** Port forwarding rules of this host. */
  forwards?: ForwardRule[];
};

export type Identity = {
  id: string;
  name: string;
  username: string | null;
  key: KeyRef | null;
  location: string | null;
  has_password: boolean;
};

export type PasswordChange = { kind: "keep" } | { kind: "set"; secret: string } | { kind: "clear" };
export type PasswordOwner =
  | { kind: "host"; id: string }
  | { kind: "group"; id: string }
  | { kind: "host_proxy"; id: string }
  | { kind: "group_proxy"; id: string }
  | { kind: "identity"; id: string };

export type IdentityInput = {
  id: string | null;
  name: string;
  username: string | null;
  key: KeyRef | null;
  location: string | null;
  password: PasswordChange;
};

export const identitiesList = () => invoke<Identity[]>("identities_list");
export const identitySave = (identity: IdentityInput) => invoke<Identity>("identity_save", { identity });
export const identityDelete = (id: string) => invoke<void>("identity_delete", { id });
export const passwordSet = (owner: PasswordOwner, secret: string) => invoke<void>("password_set", { owner, secret });
export const passwordClear = (owner: PasswordOwner) => invoke<void>("password_clear", { owner });

/** Carries out a password change from a form; "keep" and an empty new password do nothing. */
export async function applyPassword(owner: PasswordOwner, change: PasswordChange): Promise<void> {
  if (change.kind === "set" && change.secret !== "") await passwordSet(owner, change.secret);
  if (change.kind === "clear") await passwordClear(owner);
}

export type ShellCommand = { program: string; args: string[]; label: string };
export type Parity = "none" | "odd" | "even";
export type FlowControl = "none" | "software" | "hardware";
export type SerialSettings = {
  port: string;
  baud: number;
  data_bits: number;
  stop_bits: number;
  parity: Parity;
  flow: FlowControl;
};
export type SerialPortInfo = { name: string; description: string };

export type SessionTarget =
  | { kind: "host"; host_id: string }
  | { kind: "local"; shell: ShellCommand; cwd?: string | null }
  | { kind: "serial"; settings: SerialSettings };

export const localShells = () => invoke<ShellCommand[]>("local_shells");
export const localShellGet = () => invoke<ShellCommand | null>("local_shell_get");
export const localShellSet = (shell: ShellCommand | null) => invoke<void>("local_shell_set", { shell });
export const serialPorts = () => invoke<SerialPortInfo[]>("serial_ports");
export const serialLast = () => invoke<SerialSettings | null>("serial_last");
export const serialRemember = (settings: SerialSettings) => invoke<void>("serial_remember", { settings });

export type CertificateInfo = {
  key_id: string;
  principals: string[];
  /** Unix seconds. */
  valid_after: number;
  /** Unix seconds; the largest u64 means "no end". */
  valid_before: number;
  ca_fingerprint: string;
};

export type KeyInfo = {
  id: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  public_key: string;
  location?: string | null;
  /** "device": bound to this device (Windows Hello), never synced. */
  kind?: "vault" | "device";
  /** The passphrase is asked on each use. */
  protected?: boolean;
  certificate?: CertificateInfo | null;
};

export type EcdsaCurve = "nistp256" | "nistp384" | "nistp521";
export type KeySpec = { type: "ed25519" } | { type: "ecdsa"; curve: EcdsaCurve } | { type: "rsa"; bits: number };
export type SkAlgorithm = "ecdsa_p256" | "ed25519";
export type SecurityKeyKind = "fido2" | "hello";
export type InstallOutcome = "added" | "already_present";

export type EndReason =
  | "exited"
  | "lost"
  | "host_key_rejected"
  | "host_key_mismatch"
  | "auth_failed"
  | "auth_cancelled"
  | "connect_failed"
  | "error";

export type SessionStatus =
  | { state: "connecting" }
  | { state: "connected"; notice?: string }
  | { state: "reconnecting"; attempt: number; delay_secs: number }
  | { state: "ended"; reason: EndReason; code: string | null; detail: string | null };

export type HostKeyPrompt = {
  prompt_id: number;
  host_name: string;
  address: string;
  port: number;
  fingerprint: string;
};

export type AuthPrompt = {
  prompt_id: number;
  host_name: string;
  username: string;
  name: string;
  instructions: string;
  prompts: { text: string; echo: boolean }[];
  /** SSHCraft asks for the passphrase of the key with this name. */
  passphrase_for?: string;
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
export const importKey = (name: string, text: string, passphrase: string | null, savePassphrase = true) =>
  invoke<KeyInfo>("import_key", { name, text, passphrase, savePassphrase });
export const generateKey = (name: string, spec: KeySpec, passphrase: string | null) =>
  invoke<KeyInfo>("generate_key", { name, spec, passphrase });
export const securityKeySupport = () => invoke<{ fido2: boolean; hello: boolean }>("security_key_support");
export const createSecurityKey = (name: string, kind: SecurityKeyKind, algorithm: SkAlgorithm, verifyUser: boolean) =>
  invoke<KeyInfo>("create_security_key", { name, kind, algorithm, verifyUser });
export const attachCertificate = (id: string, text: string) => invoke<KeyInfo>("attach_certificate", { id, text });
export const detachCertificate = (id: string) => invoke<KeyInfo>("detach_certificate", { id });
export const rememberPassphrase = (id: string, passphrase: string) =>
  invoke<KeyInfo>("remember_passphrase", { id, passphrase });
export const protectKey = (id: string, passphrase: string) => invoke<KeyInfo>("protect_key", { id, passphrase });
export const installKey = (hostId: string, key: KeyRef) => invoke<InstallOutcome>("install_key", { hostId, key });
export const deleteKey = (id: string) => invoke<void>("delete_key", { id });
export const listAgentKeys = () => invoke<AgentKey[]>("list_agent_keys");

export type PassCliStatus = "not_installed" | "logged_out" | "ready" | "failed";
export type PassKey = { share_id: string; item_id: string; title: string };
export const passCliStatus = () => invoke<PassCliStatus>("pass_cli_status");
export const passCliListKeys = () => invoke<PassKey[]>("pass_cli_list_keys");
export const importFromPassCli = (shareId: string, itemId: string, name: string) =>
  invoke<KeyInfo>("import_from_pass_cli", { shareId, itemId, name });

export async function sessionOpen(
  target: SessionTarget,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onStatus: (status: SessionStatus) => void,
): Promise<string> {
  const data = new Channel<ArrayBuffer | number[]>();
  data.onmessage = (msg) => onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
  const status = new Channel<SessionStatus>();
  status.onmessage = onStatus;
  return invoke<string>("session_open", { target, cols, rows, onData: data, onStatus: status });
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
/** `null` cancels the sign-in. */
export const answerAuth = (promptId: number, answers: string[] | null) =>
  invoke<void>("answer_auth", { promptId, answers });

export type AppTheme = "system" | "dark" | "light";
export type HostView = "grid" | "list";
export type AppSettings = {
  language: "auto" | "en" | "de";
  app_theme: AppTheme;
  terminal_theme: string;
  font_family: string;
  font_size: number;
  host_view: HostView;
  /** One of `TERMINAL_TYPES` (settings/terminal.ts). */
  terminal_type: string;
  clipboard_osc52: "write" | "off";
  /** Shortcuts that differ from the defaults; an empty list unbinds the action. */
  shortcuts: Record<string, string[]>;
  /** Typed commands are kept (sealed) for the sidebar and completion. */
  history: boolean;
  autocomplete: boolean;
  /** Record sessions unless a host or group says otherwise. */
  session_logs: boolean;
  /** Days a finished log is kept; 0 keeps it. One of `LOG_RETENTION_DAYS`. */
  log_retention_days: number;
};
export const LOG_RETENTION_DAYS = [0, 7, 30, 90, 365] as const;
export const getSettings = () => invoke<AppSettings>("get_settings");
/** Returns what Rust stored, with values kept in their allowed ranges. */
export const saveSettings = (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings });
export const systemLocale = () => invoke<string | null>("system_locale");
export type TemplatePane = {
  target: SessionTarget;
  title: string;
  cwd?: string | null;
  command?: string | null;
};
export type WorkspaceTemplate = {
  id: string;
  name: string;
  layout: "split" | "focus";
  col_sizes: number[];
  row_sizes: number[];
  panes: TemplatePane[];
};
/** Templates are kept on this device only. */
export const workspaceTemplates = () => invoke<WorkspaceTemplate[]>("workspace_templates");
export const saveWorkspaceTemplate = (template: WorkspaceTemplate) =>
  invoke<WorkspaceTemplate>("save_workspace_template", { template });
export const deleteWorkspaceTemplate = (id: string) => invoke<void>("delete_workspace_template", { id });
/** Opens an http(s) link in the default browser; Rust checks it again. */
export const openLink = (url: string) => invoke<void>("open_link", { url });
export type ForwardState =
  | { state: "starting" }
  | { state: "running"; bound_port: number; connections: number; refused: number; notice: string | null }
  | { state: "reconnecting"; attempt: number; delay_secs: number }
  | { state: "stopped" }
  | { state: "failed"; code: string | null; detail: string | null };
export type ForwardStatus = { rule_id: string; host_id: string } & ForwardState;
export const forwardStart = (hostId: string, ruleId: string) => invoke<void>("forward_start", { hostId, ruleId });
export const forwardStop = (hostId: string, ruleId: string) => invoke<void>("forward_stop", { hostId, ruleId });
export const forwardStatuses = () => invoke<ForwardStatus[]>("forward_statuses");
export type Snippet = {
  id: string;
  name: string;
  /** Empty for none. */
  package: string;
  content: string;
  default_targets: string[];
};
export type HistoryEntry = { command: string; uses: number; last_used: number };
export const snippetsList = () => invoke<Snippet[]>("snippets_list");
export const snippetSave = (snippet: Snippet) => invoke<Snippet>("snippet_save", { snippet });
export const snippetDelete = (id: string) => invoke<void>("snippet_delete", { id });
export const historyRecord = (scope: string, command: string) => invoke<void>("history_record", { scope, command });
export const historyList = (scope: string) => invoke<HistoryEntry[]>("history_list", { scope });
export const historyClear = (scope: string | null) => invoke<void>("history_clear", { scope });
/** Names in `dir` on the session's machine; directories end in "/". */
export const sessionListDir = (id: string, dir: string, prefix: string) =>
  invoke<string[]>("session_list_dir", { id, dir, prefix });
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
  /** Session logs sync too. */
  sync_logs: boolean;
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

// Session logs.
export type LogKind = "ssh" | "serial" | "local";
export type MarkKind = "bookmark" | "comment";
/** A bookmark or comment at a byte position of the recorded output. */
export type LogMark = { id: string; offset: number; kind: MarkKind; text: string; created_at: number };
export type LogMeta = {
  id: string;
  title: string;
  kind: LogKind;
  host_id: string | null;
  started_at: number;
  /** null while it is being recorded. */
  ended_at: number | null;
  size: number;
  truncated: boolean;
  marks: LogMark[];
};
/** A log with the sync server it lives at (null: this device only). */
export type LogSummary = LogMeta & { location: string | null };
export type LogHit = { log_id: string; line: number; snippet: string };
export type LogExportFormat = "text" | "raw";
export const logsList = () => invoke<LogSummary[]>("logs_list");
/** The recorded bytes. */
export const logRead = async (id: string) => new Uint8Array(await invoke<ArrayBuffer>("log_read", { id }));
/** Adds a mark to what the log has now (marks set meanwhile, e.g. from the terminal, stay). */
export const logAddMark = (id: string, mark: LogMark) => invoke<LogMeta>("log_add_mark", { id, mark });
export const logRemoveMark = (id: string, mark: string) => invoke<LogMeta>("log_remove_mark", { id, mark });
export const logsDelete = (ids: string[]) => invoke<void>("logs_delete", { ids });
export const logsSearch = (query: string) => invoke<LogHit[]>("logs_search", { query });
/** Asks for the file in a dialog of the app; false when it was cancelled. */
export const logExport = (id: string, format: LogExportFormat) => invoke<boolean>("log_export", { id, format });
export const sessionBookmark = (id: string, text: string) => invoke<LogMark>("session_bookmark", { id, text });
export const sessionRecording = (id: string) => invoke<boolean>("session_recording", { id });
export const setServerLogs = (id: string, on: boolean) => invoke<void>("set_server_logs", { id, on });

// AI agents.
export const AGENT_CATEGORIES = ["list", "connect", "edit", "delete", "keys", "share", "sync"] as const;
export type AgentCategory = (typeof AGENT_CATEGORIES)[number];
export type AgentLevel = "allow" | "ask" | "deny";
/** Levels that differ from the defaults; ids of hosts and groups agents never see. */
export type AgentPolicy = { levels: Partial<Record<AgentCategory, AgentLevel>>; hidden: string[] };
export const AGENT_DEFAULTS: Record<AgentCategory, AgentLevel> = {
  list: "allow",
  connect: "ask",
  edit: "ask",
  delete: "deny",
  keys: "ask",
  share: "deny",
  sync: "deny",
};
export type AgentOutcome =
  | "allowed"
  | "asked_allowed"
  | "asked_denied"
  | "denied"
  | "hidden"
  | "failed"
  | "cancelled";
export type AgentLogEntry = {
  id: string;
  at: number;
  agent: string;
  action: string;
  target: string;
  detail: string;
  outcome: AgentOutcome;
};
/** A question about an agent's action, answered in this window only. */
export type AgentQuestion = { prompt_id: number; agent: string; action: string; target: string; detail: string };
export const agentPolicyGet = () => invoke<AgentPolicy>("agent_policy_get");
/** One change at a time on the stored policy (a window that has not loaded it cannot overwrite it). */
export const agentSetHidden = (id: string, hidden: boolean) => invoke<AgentPolicy>("agent_set_hidden", { id, hidden });
export const agentSetLevel = (category: AgentCategory, level: AgentLevel) =>
  invoke<AgentPolicy>("agent_set_level", { category, level });
export const agentLogList = () => invoke<AgentLogEntry[]>("agent_log_list");
export const agentLogClear = () => invoke<void>("agent_log_clear");
export const answerAgent = (promptId: number, allow: boolean) => invoke<void>("answer_agent", { promptId, allow });
