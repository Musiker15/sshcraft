import { describe, expect, test } from "vitest";
import { FormError } from "../i18n/errors";
import { emptyConnection, fromConnectionOptions, toConnectionOptions } from "./connectionForm";

const base = { address: null, port: null, username: null, key: null };

function codeOf(run: () => unknown): string | null {
  try {
    run();
    return null;
  } catch (e) {
    return e instanceof FormError ? e.code : "other";
  }
}

describe("proxy", () => {
  test("inherit, direct and http go back and forth", () => {
    expect(toConnectionOptions(emptyConnection).proxy).toBeNull();
    expect(toConnectionOptions({ ...emptyConnection, proxyMode: "direct" }).proxy).toEqual({ kind: "direct" });
    const http = toConnectionOptions({
      ...emptyConnection,
      proxyMode: "http",
      proxyAddress: " proxy.example.org ",
      proxyPort: "3128",
      proxyUser: "",
    }).proxy;
    expect(http).toEqual({ kind: "http", address: "proxy.example.org", port: 3128, username: null });
    const back = fromConnectionOptions({ ...base, proxy: http });
    expect([back.proxyMode, back.proxyAddress, back.proxyPort]).toEqual(["http", "proxy.example.org", "3128"]);
  });

  test("a proxy needs address and port", () => {
    expect(codeOf(() => toConnectionOptions({ ...emptyConnection, proxyMode: "socks5", proxyPort: "1080" }))).toBe(
      "proxy_address_required",
    );
    expect(codeOf(() => toConnectionOptions({ ...emptyConnection, proxyMode: "socks5", proxyAddress: "p" }))).toBe(
      "proxy_port_required",
    );
  });
});

describe("jump hosts", () => {
  test("inherit gives null, none gives an empty list", () => {
    expect(toConnectionOptions(emptyConnection).jump_hosts).toBeNull();
    expect(toConnectionOptions({ ...emptyConnection, jumpMode: "none", jumps: ["x"] }).jump_hosts).toEqual([]);
    expect(toConnectionOptions({ ...emptyConnection, jumpMode: "list", jumps: ["a", "b"] }).jump_hosts).toEqual([
      "a",
      "b",
    ]);
    expect(fromConnectionOptions({ ...base, jump_hosts: [] }).jumpMode).toBe("none");
    expect(fromConnectionOptions({ ...base }).jumpMode).toBe("inherit");
  });
});

describe("environment", () => {
  test("rows with an invalid name are dropped when saving", () => {
    const env = toConnectionOptions({
      ...emptyConnection,
      env: [
        { name: "LANG", value: "de_DE.UTF-8" },
        { name: "1BAD", value: "x" },
        { name: "A-B", value: "x" },
        { name: " TZ ", value: "UTC" },
      ],
    }).env;
    expect(env).toEqual([
      { name: "LANG", value: "de_DE.UTF-8" },
      { name: "TZ", value: "UTC" },
    ]);
  });
});

test("agent forwarding has three states", () => {
  expect(toConnectionOptions(emptyConnection).agent_forwarding).toBeNull();
  expect(toConnectionOptions({ ...emptyConnection, agent: "on" }).agent_forwarding).toBe(true);
  expect(toConnectionOptions({ ...emptyConnection, agent: "off" }).agent_forwarding).toBe(false);
  expect(fromConnectionOptions({ ...base, agent_forwarding: false }).agent).toBe("off");
});

test("recording has three states", () => {
  expect(toConnectionOptions(emptyConnection).session_log).toBeNull();
  expect(toConnectionOptions({ ...emptyConnection, record: "on" }).session_log).toBe(true);
  expect(toConnectionOptions({ ...emptyConnection, record: "off" }).session_log).toBe(false);
  expect(fromConnectionOptions({ ...base, session_log: false }).record).toBe("off");
  expect(fromConnectionOptions({ ...base, session_log: true }).record).toBe("on");
  expect(fromConnectionOptions(base).record).toBe("inherit");
});
