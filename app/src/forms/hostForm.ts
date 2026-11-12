// Converts between form text fields and ConnectionSettings. Empty means "inherit from group".
import type { ConnectionSettings, KeyRef } from "../api";
import { FormError } from "../i18n/errors";

export type SettingsFormValues = {
  address: string;
  port: string;
  username: string;
  /** "" (inherit), "stored:<uuid>" or "agent:<fingerprint>". */
  keyChoice: string;
};

function text(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export function parsePort(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) throw new FormError("port_not_number");
  const port = Number(trimmed);
  if (port < 1 || port > 65535) throw new FormError("port_range");
  return port;
}

export function keyChoice(key: KeyRef | null): string {
  if (!key) return "";
  return key.kind === "stored" ? `stored:${key.id}` : `agent:${key.fingerprint}`;
}

export function parseKeyChoice(choice: string): KeyRef | null {
  if (choice === "") return null;
  // Split at the first colon only: fingerprints contain colons themselves.
  const colon = choice.indexOf(":");
  const kind = choice.slice(0, colon);
  const value = choice.slice(colon + 1);
  if (colon > 0 && value !== "") {
    if (kind === "stored") return { kind: "stored", id: value };
    if (kind === "agent") return { kind: "agent", fingerprint: value };
  }
  throw new FormError("unknown_key_choice");
}

export function toConnectionSettings(form: SettingsFormValues): ConnectionSettings {
  return {
    address: text(form.address),
    port: parsePort(form.port),
    username: text(form.username),
    key: parseKeyChoice(form.keyChoice),
  };
}

export function fromConnectionSettings(settings: ConnectionSettings): SettingsFormValues {
  return {
    address: settings.address ?? "",
    port: settings.port === null ? "" : String(settings.port),
    username: settings.username ?? "",
    keyChoice: keyChoice(settings.key),
  };
}
