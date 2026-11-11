// Converts between form text fields and ConnectionSettings. Empty means "inherit from group".
import type { ConnectionSettings } from "../api";

export type SettingsFormValues = {
  address: string;
  port: string;
  username: string;
  keyId: string;
};

function text(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export function parsePort(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) throw new Error("Der Port muss eine Zahl sein.");
  const port = Number(trimmed);
  if (port < 1 || port > 65535) throw new Error("Der Port muss zwischen 1 und 65535 liegen.");
  return port;
}

export function toConnectionSettings(form: SettingsFormValues): ConnectionSettings {
  const keyId = text(form.keyId);
  return {
    address: text(form.address),
    port: parsePort(form.port),
    username: text(form.username),
    key: keyId ? { kind: "stored", id: keyId } : null,
  };
}

export function fromConnectionSettings(settings: ConnectionSettings): SettingsFormValues {
  return {
    address: settings.address ?? "",
    port: settings.port === null ? "" : String(settings.port),
    username: settings.username ?? "",
    keyId: settings.key?.id ?? "",
  };
}
