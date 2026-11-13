import { expect, test } from "vitest";
import type { Host } from "../api";
import type { SessionTab } from "../state/tabs";
import { defaultPanes, planNewTabs, snippetInput } from "./snippetRun";

test("line ends become Enter; running ends with Enter, pasting does not", () => {
  expect(snippetInput("a\r\nb\n", true)).toBe("a\rb\r");
  expect(snippetInput("a\r\nb\n", false)).toBe("a\rb");
  expect(snippetInput("uptime", true)).toBe("uptime\r");
});

const host = (id: string): Host => ({
  id,
  name: id,
  group_id: null,
  settings: { address: null, port: null, username: null, key: null },
});

test("new connections go into tabs of at most 16, deleted hosts are counted", () => {
  const hosts = Array.from({ length: 20 }, (_, i) => host(`h${i}`));
  const plan = planNewTabs(hosts, [...hosts.map((h) => h.id), "gone"]);
  expect(plan.tabs.map((t) => t.length)).toEqual([16, 4]);
  expect(plan.tabs[0][0]).toBe("h0");
  expect(plan.missing).toBe(1);
  expect(planNewTabs(hosts, [])).toEqual({ tabs: [], missing: 0 });
});

test("open panes connected to a default target are picked, panes without a session are not", () => {
  const tab: SessionTab = {
    kind: "session",
    id: "t",
    focusedId: "a",
    layout: "split",
    broadcast: false,
    colSizes: [],
    rowSizes: [],
    panes: [
      { id: "a", target: { kind: "host", host_id: "h1" }, title: "a", sessionId: "s-a", status: { state: "connected" }, activity: "none" },
      { id: "b", target: { kind: "host", host_id: "h1" }, title: "b", status: { state: "connecting" }, activity: "none" },
      { id: "c", target: { kind: "host", host_id: "h2" }, title: "c", sessionId: "s-c", status: { state: "connected" }, activity: "none" },
      { id: "p", picker: true },
    ],
  };
  expect(defaultPanes([tab], ["h1"])).toEqual(["a"]);
  expect(defaultPanes([tab], [])).toEqual([]);
});
