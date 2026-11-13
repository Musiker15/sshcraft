// Hidden hosts and groups in the agent policy.
import type { AgentPolicy } from "../api";

export const emptyPolicy: AgentPolicy = { levels: {}, hidden: [] };

/** The policy with `id` hidden from agents, or shown again. */
export function withHidden(policy: AgentPolicy, id: string, hidden: boolean): AgentPolicy {
  const rest = policy.hidden.filter((h) => h !== id);
  return { ...policy, hidden: hidden ? [...rest, id] : rest };
}
