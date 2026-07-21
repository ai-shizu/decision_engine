import type { KnowledgePolicyState } from "./parseKnowledgePolicy";

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
