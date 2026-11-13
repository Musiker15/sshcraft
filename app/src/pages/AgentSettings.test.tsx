import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AgentPolicy, Group, Host } from "../api";
import { withHidden } from "../agents/policy";
import { needsCareful, visibleText } from "../agents/visible";
import { AgentConfirmDialog } from "../components/AgentConfirmDialog";
import en from "../i18n/en.json";
import { I18nProvider } from "../i18n/i18n";
import { AgentSettings } from "./AgentSettings";

const state = vi.hoisted(() => ({
  policy: { levels: {}, hidden: [] } as AgentPolicy,
  saved: [] as AgentPolicy[],
  calls: [] as { command: string; args: unknown }[],
  cleared: 0,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    state.calls.push({ command, args });
    if (command === "agent_policy_get") return state.policy;
    if (command === "agent_set_level") {
      const next = {
        ...state.policy,
        levels: { ...state.policy.levels, [args!.category as string]: args!.level },
      } as AgentPolicy;
      state.policy = next;
      state.saved.push(next);
      return next;
    }
    if (command === "agent_set_hidden") {
      const rest = state.policy.hidden.filter((h) => h !== args!.id);
      const next = { ...state.policy, hidden: args!.hidden ? [...rest, args!.id as string] : rest };
      state.policy = next;
      state.saved.push(next);
      return next;
    }
    if (command === "agent_log_list")
      return state.cleared > 0
        ? []
        : [
            {
              id: "e1",
              at: Date.UTC(2026, 10, 14, 2, 0),
              agent: "claude-code",
              action: "exec",
              target: "web (10.0.0.1)",
              detail: "uptime",
              outcome: "asked_allowed",
            },
          ];
    if (command === "agent_log_clear") {
      state.cleared += 1;
      return null;
    }
    return null;
  }),
}));

const hosts: Host[] = [
  { id: "h1", name: "prod-db", group_id: null, settings: { address: null, port: null, username: null, key: null } },
];
const groups: Group[] = [];

beforeEach(() => {
  state.policy = { levels: { connect: "allow" }, hidden: ["h1"] };
  state.saved = [];
  state.calls = [];
  state.cleared = 0;
});
afterEach(cleanup);

async function show() {
  render(
    <I18nProvider language="en">
      <AgentSettings hosts={hosts} groups={groups} />
    </I18nProvider>,
  );
  await screen.findByText("prod-db");
}

test("levels show the stored ones and the defaults, and are saved", async () => {
  await show();
  const connect = screen.getByLabelText(en["agents.category.connect"]) as HTMLSelectElement;
  expect(connect.value).toBe("allow");
  const remove = screen.getByLabelText(en["agents.category.delete"]) as HTMLSelectElement;
  expect(remove.value).toBe("deny");
  fireEvent.change(remove, { target: { value: "ask" } });
  await vi.waitFor(() => expect(state.saved[state.saved.length - 1]?.levels.delete).toBe("ask"));
  expect(state.saved[state.saved.length - 1]?.levels.connect).toBe("allow");
});

test("a hidden host can be shown again", async () => {
  await show();
  fireEvent.click(screen.getByRole("button", { name: en["agents.show"] }));
  await vi.waitFor(() => expect(state.saved[state.saved.length - 1]?.hidden).toEqual([]));
  // One change, not the whole policy: a stale window cannot overwrite the rest.
  expect(state.calls.find((c) => c.command === "agent_set_hidden")?.args).toEqual({ id: "h1", hidden: false });
});

test("the log lists actions and can be cleared", async () => {
  await show();
  expect(await screen.findByText("uptime")).toBeTruthy();
  expect(screen.getByText(en["agents.outcome.asked_allowed"])).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: en["agents.logClear"] }));
  await vi.waitFor(() => expect(screen.getByText(en["agents.logEmpty"])).toBeTruthy());
});

test("hiding adds once and showing removes", () => {
  const policy: AgentPolicy = { levels: {}, hidden: ["a"] };
  expect(withHidden(policy, "a", true).hidden).toEqual(["a"]);
  expect(withHidden(policy, "b", true).hidden).toEqual(["a", "b"]);
  expect(withHidden(policy, "a", false).hidden).toEqual([]);
});

test("hidden and reordering characters become visible", () => {
  expect(visibleText("ls\u202Eexe.txt")).toBe("ls\u27E8U+202E\u27E9exe.txt");
  expect(visibleText("a\u0007b\nc")).toBe("a\u27E8U+0007\u27E9b\nc");
  expect(needsCareful("uptime")).toBe(false);
  expect(needsCareful("echo ok\nrm -rf /")).toBe(true);
});

test("the question shows agent, action, target and command, and starts on deny", async () => {
  const onAnswer = vi.fn();
  render(
    <I18nProvider language="en">
      <AgentConfirmDialog
        question={{ prompt_id: 4, agent: "codex", action: "exec", target: "web (10.0.0.1)", detail: "rm -rf /tmp/x" }}
        onAnswer={onAnswer}
      />
    </I18nProvider>,
  );
  expect(screen.getByText("codex wants to: run a command.")).toBeTruthy();
  expect(screen.getByText("rm -rf /tmp/x")).toBeTruthy();
  // A click right after the question appeared does nothing.
  const allow = screen.getByRole("button", { name: en["agents.allow"] });
  fireEvent.click(allow);
  expect(onAnswer).not.toHaveBeenCalled();
  await vi.waitFor(() => expect(allow.hasAttribute("disabled")).toBe(false), { timeout: 2000 });
  fireEvent.click(allow);
  expect(onAnswer).toHaveBeenCalledWith(true);
});
