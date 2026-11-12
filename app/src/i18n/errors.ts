// Errors from the Rust core arrive as a code plus an untranslated technical detail.
import type { Translate } from "./i18n";

export type CommandError = { code: string; detail: string | null };

/** A check in the UI failed; `code` is translated like a code from Rust (`errors.<code>`). */
export class FormError extends Error {
  readonly code: string;
  readonly detail = null;
  constructor(code: string) {
    super(code);
    this.code = code;
  }
}

function asCommandError(e: unknown): CommandError | null {
  if (e && typeof e === "object" && "code" in e && typeof (e as CommandError).code === "string") {
    const detail = (e as { detail?: unknown }).detail;
    return { code: (e as CommandError).code, detail: typeof detail === "string" ? detail : null };
  }
  return null;
}

export function errorText(t: Translate, e: unknown): string {
  const error = asCommandError(e);
  if (error) {
    const key = `errors.${error.code}`;
    const text = t(key);
    if (text !== key) return text;
  }
  return t("errors.unknown");
}

export function errorDetail(e: unknown): string | null {
  const error = asCommandError(e);
  if (error) return error.detail;
  if (e instanceof Error) return e.message;
  return e == null ? null : String(e);
}
