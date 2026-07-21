/**
 * Pure GD setup state (Coliseum / ARENA_GD). No React.
 * Agents = participants excluding the user.
 */

export type GdUserRole =
  | "facilitator"
  | "scribe"
  | "timekeeper"
  | "none";

export type GdArchetype =
  | "logical_leader"
  | "aggressive_crusher"
  | "passive_harmonizer"
  | "framework_zombie"
  | "random_mutation";

export type GdAgentProfile = {
  id: string;
  label: string;
  archetype: GdArchetype;
  /** Hostility toward user rebuttal [0,1]. */
  hostility: number;
  /** Competence / intelligence [0,1]. */
  competence: number;
};

export type GdSetupConfig = {
  theme: string;
  /** Total seats including user (4–6). */
  participants: number;
  timeLimitMin: number;
  userRole: GdUserRole;
  agents: GdAgentProfile[];
};

export type GdPhase = "setup" | "armed";

export const GD_PARTICIPANT_OPTIONS = [4, 5, 6] as const;

export const GD_USER_ROLE_OPTIONS: { id: GdUserRole; label: string }[] = [
  { id: "none", label: "指定なし" },
  { id: "facilitator", label: "ファシリテーター" },
  { id: "scribe", label: "書記" },
  { id: "timekeeper", label: "タイムキーパー" },
];

export const GD_ARCHETYPE_OPTIONS: { id: GdArchetype; label: string }[] = [
  { id: "logical_leader", label: "論理・支配型" },
  { id: "aggressive_crusher", label: "攻撃・クラッシャー" },
  { id: "passive_harmonizer", label: "受動・調和型" },
  { id: "framework_zombie", label: "フレームワーク固執" },
  { id: "random_mutation", label: "予測不能" },
];

const DEFAULT_ARCHETYPES: GdArchetype[] = [
  "aggressive_crusher",
  "passive_harmonizer",
  "logical_leader",
  "framework_zombie",
  "random_mutation",
];

export function agentCountFromParticipants(participants: number): number {
  const n = Math.max(4, Math.min(6, Math.trunc(participants)));
  return Math.max(1, n - 1);
}

export function makeAgentProfile(index: number, prev?: GdAgentProfile): GdAgentProfile {
  const letter = String.fromCharCode(65 + index);
  return {
    id: prev?.id ?? `agent-${letter.toLowerCase()}`,
    label: `Agent ${letter}`,
    archetype: prev?.archetype ?? DEFAULT_ARCHETYPES[index % DEFAULT_ARCHETYPES.length],
    hostility: prev?.hostility ?? 0.55,
    competence: prev?.competence ?? 0.6,
  };
}

export function syncAgentsToParticipants(
  participants: number,
  current: GdAgentProfile[],
): GdAgentProfile[] {
  const need = agentCountFromParticipants(participants);
  const next: GdAgentProfile[] = [];
  for (let i = 0; i < need; i += 1) {
    next.push(makeAgentProfile(i, current[i]));
  }
  return next;
}

export function initialGdSetupConfig(): GdSetupConfig {
  const participants = 5;
  return {
    theme: "",
    participants,
    timeLimitMin: 30,
    userRole: "none",
    agents: syncAgentsToParticipants(participants, []),
  };
}

export function gdSetupReady(config: GdSetupConfig): boolean {
  return (
    config.theme.trim().length > 0 &&
    config.participants >= 4 &&
    config.participants <= 6 &&
    config.timeLimitMin > 0 &&
    config.agents.length === agentCountFromParticipants(config.participants)
  );
}

/** Map archetype → legacy gd_sim trait string (prompt injection later). */
export function archetypeToTrait(archetype: GdArchetype): string {
  switch (archetype) {
    case "logical_leader":
      return "論理的";
    case "aggressive_crusher":
      return "クラッシャー";
    case "passive_harmonizer":
      return "協調型";
    case "framework_zombie":
      return "クラウザー";
    case "random_mutation":
      return "予測不能・ランダム変異";
    default:
      return "協調型";
  }
}
