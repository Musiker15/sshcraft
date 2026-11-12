import type { EcdsaCurve, KeySpec } from "../api";
import { FormError } from "../i18n/errors";

export type KeyType = "ed25519" | "ecdsa" | "rsa";

/** The "new key" form as typed. */
export type GenerateForm = {
  name: string;
  type: KeyType;
  curve: EcdsaCurve;
  bits: 3072 | 4096;
  passphrase: string;
  repeat: string;
  /** Save the passphrase (no question on use) instead of asking for it each time. */
  save: boolean;
};

export type GenerateRequest = { name: string; spec: KeySpec; passphrase: string | null };

/** What `generate_key` gets. A saved passphrase protects nothing, so it does not go along. */
export function generateRequest(form: GenerateForm): GenerateRequest {
  const name = form.name.trim();
  if (!name) throw new FormError("name_required");
  if (form.passphrase !== form.repeat) throw new FormError("passphrase_mismatch");
  const spec: KeySpec =
    form.type === "ecdsa"
      ? { type: "ecdsa", curve: form.curve }
      : form.type === "rsa"
        ? { type: "rsa", bits: form.bits }
        : { type: "ed25519" };
  const passphrase = form.passphrase && !form.save ? form.passphrase : null;
  return { name, spec, passphrase };
}
