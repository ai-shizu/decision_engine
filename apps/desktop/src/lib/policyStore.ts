import type { KnowledgePolicyState } from "./parseKnowledgePolicy";

/** Default consent state (fail-closed). */
export const DEFAULT_KNOWLEDGE_POLICY: KnowledgePolicyState = {
  schema: "knowledge_policy.v1",
  enabled: false,
};

export function applyPolicyEnabled(
  current: KnowledgePolicyState,
  enabled: boolean,
): KnowledgePolicyState {
  return {
    schema: current.schema,
    enabled,
  };
}

export function policySetRequest(enabled: boolean): { enabled: boolean } {
  return { enabled };
}
