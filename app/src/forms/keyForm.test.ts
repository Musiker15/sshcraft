import { expect, test } from "vitest";
import { FormError } from "../i18n/errors";
import { generateRequest, type GenerateForm } from "./keyForm";

const form = (patch: Partial<GenerateForm>): GenerateForm => ({
  name: "deploy",
  type: "ed25519",
  curve: "nistp256",
  bits: 3072,
  passphrase: "",
  repeat: "",
  save: true,
  ...patch,
});

test("each type becomes its spec", () => {
  expect(generateRequest(form({}))).toEqual({ name: "deploy", spec: { type: "ed25519" }, passphrase: null });
  expect(generateRequest(form({ type: "ecdsa", curve: "nistp384" })).spec).toEqual({ type: "ecdsa", curve: "nistp384" });
  expect(generateRequest(form({ type: "rsa", bits: 4096 })).spec).toEqual({ type: "rsa", bits: 4096 });
});

test("a passphrase goes along only when it is not saved", () => {
  expect(generateRequest(form({ passphrase: "pw", repeat: "pw", save: true })).passphrase).toBeNull();
  expect(generateRequest(form({ passphrase: "pw", repeat: "pw", save: false })).passphrase).toBe("pw");
});

test("the repeated passphrase must match", () => {
  expect(() => generateRequest(form({ passphrase: "a", repeat: "b", save: false }))).toThrow(FormError);
  try {
    generateRequest(form({ passphrase: "a", repeat: "b", save: false }));
  } catch (e) {
    expect((e as FormError).code).toBe("passphrase_mismatch");
  }
});

test("a name is required", () => {
  expect(() => generateRequest(form({ name: "  " }))).toThrow(FormError);
});
