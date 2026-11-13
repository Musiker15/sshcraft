import { expect, test } from "vitest";
import type { ForwardRule } from "../api";
import { emptyDraft, isExposed, sshEquivalent, toRule, validateDraft } from "./forwardForm";

const rule = (patch: Partial<ForwardRule>): ForwardRule => ({
  id: "r",
  name: "",
  kind: "local",
  bind_address: "127.0.0.1",
  bind_port: 8080,
  target_host: "db",
  target_port: 5432,
  ...patch,
});

test("drafts start with the defaults of their kind", () => {
  expect(emptyDraft("local")).toMatchObject({ kind: "local", bind_address: "127.0.0.1", bind_port: "", target_host: "" });
  expect(emptyDraft("remote").bind_address).toBe("localhost");
  expect(emptyDraft("dynamic")).toMatchObject({ bind_address: "127.0.0.1", bind_port: "1080" });
});

test("a complete local draft is valid and becomes a rule", () => {
  const draft = { ...emptyDraft("local"), bind_port: "15432", target_host: " db.internal ", target_port: "5432", name: " DB " };
  expect(validateDraft(draft)).toBeNull();
  expect(toRule(draft, "id-1")).toEqual({
    id: "id-1",
    name: "DB",
    kind: "local",
    bind_address: "127.0.0.1",
    bind_port: 15432,
    target_host: "db.internal",
    target_port: 5432,
  });
});

test("a dynamic draft has no target", () => {
  const draft = { ...emptyDraft("dynamic"), target_host: "ignored", target_port: "99" };
  expect(validateDraft(draft)).toBeNull();
  expect(toRule(draft, "d")).toMatchObject({ target_host: "", target_port: 0, bind_port: 1080 });
});

test.each([
  [{ bind_port: "0" }, "forwarding.error.bindPort"],
  [{ bind_port: "65536" }, "forwarding.error.bindPort"],
  [{ bind_port: "80a" }, "forwarding.error.bindPort"],
  [{ bind_address: "" }, "forwarding.error.bindAddress"],
  [{ bind_address: "127.0.0.1 " }, "forwarding.error.bindAddress"],
  [{ target_host: "" }, "forwarding.error.targetHost"],
  [{ target_host: "a b" }, "forwarding.error.targetHost"],
  [{ target_port: "" }, "forwarding.error.targetPort"],
  [{ name: "n".repeat(101) }, "forwarding.error.name"],
])("%j is refused with %s", (patch, key) => {
  const draft = { ...emptyDraft("local"), bind_port: "8080", target_host: "db", target_port: "5432", ...patch };
  expect(validateDraft(draft)).toBe(key);
});

test("the ssh command line says the same", () => {
  expect(sshEquivalent(rule({}))).toBe("-L 127.0.0.1:8080:db:5432");
  expect(
    sshEquivalent(rule({ kind: "remote", bind_address: "localhost", bind_port: 9000, target_host: "127.0.0.1", target_port: 3000 })),
  ).toBe("-R localhost:9000:127.0.0.1:3000");
  expect(sshEquivalent(rule({ kind: "dynamic", bind_port: 1080, target_host: "", target_port: 0 }))).toBe(
    "-D 127.0.0.1:1080",
  );
  expect(sshEquivalent(rule({ bind_address: "::1", target_host: "fe80::1" }))).toBe("-L [::1]:8080:[fe80::1]:5432");
});

test("listening beyond this machine is exposed", () => {
  expect(isExposed(rule({ bind_address: "0.0.0.0" }))).toBe(true);
  expect(isExposed(rule({ bind_address: "192.168.1.5" }))).toBe(true);
  expect(isExposed(rule({ bind_address: "*" }))).toBe(true);
  expect(isExposed(rule({ bind_address: "127.0.0.1" }))).toBe(false);
  expect(isExposed(rule({ bind_address: "127.0.0.2" }))).toBe(false);
  expect(isExposed(rule({ bind_address: "::1" }))).toBe(false);
  expect(isExposed(rule({ bind_address: "localhost" }))).toBe(false);
});
