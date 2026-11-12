import { expect, test } from "vitest";
import type { Host, KeyInfo } from "../../api";
import { missingKey, statusTone } from "./syncView";

test("status tones", () => {
  expect(statusTone({ state: "idle" })).toBe("ok");
  expect(statusTone({ state: "syncing" })).toBe("busy");
  expect(statusTone({ state: "offline", retry_in_secs: 4 })).toBe("offline");
  expect(statusTone({ state: "revoked" })).toBe("revoked");
  expect(statusTone({ state: "error", detail: "x" })).toBe("error");
  expect(statusTone({ state: "off" })).toBe("off");
});

const host = (key: Host["settings"]["key"]): Host => ({
  id: "h",
  name: "web",
  group_id: null,
  settings: { address: "10.0.0.1", port: null, username: "root", key },
});
const keys: KeyInfo[] = [{ id: "k1", name: "deploy", algorithm: "ssh-ed25519", fingerprint: "SHA256:x", public_key: "" }];

test("a stored key that is not on this device is missing", () => {
  expect(missingKey(host({ kind: "stored", id: "k1" }), keys)).toBe(false);
  expect(missingKey(host({ kind: "stored", id: "other" }), keys)).toBe(true);
  expect(missingKey(host({ kind: "agent", fingerprint: "SHA256:y" }), keys)).toBe(false);
  expect(missingKey(host(null), keys)).toBe(false);
});
