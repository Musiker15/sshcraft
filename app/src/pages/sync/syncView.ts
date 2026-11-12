import type { Host, KeyInfo, SyncStatusView } from "../../api";

export type Tone = "ok" | "busy" | "offline" | "revoked" | "error" | "off";

/** The colour class of a server's status dot. */
export function statusTone(status: SyncStatusView): Tone {
  switch (status.state) {
    case "idle":
      return "ok";
    case "syncing":
      return "busy";
    case "offline":
      return "offline";
    case "revoked":
      return "revoked";
    case "error":
      return "error";
    case "off":
      return "off";
  }
}

/** A host points to a stored key this device does not have (key sync off, or not synced yet). */
export function missingKey(host: Host, keys: KeyInfo[]): boolean {
  const key = host.settings.key;
  return key?.kind === "stored" && !keys.some((k) => k.id === key.id);
}

/** The account key as a picture for `<img>`; the SVG comes from Rust, never from the server. */
export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
