//! GD multi-agent system-prompt builder (Inner Coliseum). Pure — no React, no
//! IPC. Output feeds `generate({ prompt })` in llm.ts directly: llm_generate
//! already accepts a raw frontend-built prompt, so no Rust change is needed.
//!
//! SPEAK-DSL: one line per utterance, head `@<letter>>`; letters A–E map 1:1
//! to PARTICIPANT_A..E (see gdStreamParser.ts). The model plays only the
//! agents — USER is typed by the human and must never be generated.

import {
  GD_ARCHETYPE_OPTIONS,
  GD_USER_ROLE_OPTIONS,
  type GdAgentProfile,
  type GdSetupConfig,
} from "./gdSetupState";
import type { GdTranscriptMessage } from "./gdStreamParser";

/** Coliseum seats top out at 6 (5 agents + user) — see GD_PARTICIPANT_OPTIONS. */
export const GD_MAX_AGENTS = 5;

/** A..E handle for the Nth agent (parser maps back to PARTICIPANT_<letter>). */
export function gdAgentLetter(index: number): string {
  return String.fromCharCode(65 + index);
}

/** Letters actually in play for this config, e.g. ["A","B","C"] for 4 seats. */
export function gdValidLetters(config: GdSetupConfig): string[] {
  return config.agents.slice(0, GD_MAX_AGENTS).map((_, i) => gdAgentLetter(i));
}

function archetypeLabel(archetype: GdAgentProfile["archetype"]): string {
  return (
    GD_ARCHETYPE_OPTIONS.find((o) => o.id === archetype)?.label ?? "標準型"
  );
}

function userRoleLabel(role: GdSetupConfig["userRole"]): string {
  return GD_USER_ROLE_OPTIONS.find((o) => o.id === role)?.label ?? "指定なし";
}

/** 0..1 float (GdAgentProfile) → 0..100 int (reads cleaner to a 1.5B). */
function pct(unit: number): number {
  const n = Number.isFinite(unit) ? unit : 0;
  return Math.max(0, Math.min(100, Math.round(n * 100)));
}

function rosterBlock(agents: GdAgentProfile[]): string {
  return agents
    .slice(0, GD_MAX_AGENTS)
    .map((ag, i) => {
      const letter = gdAgentLetter(i);
      return `@${letter} ${ag.label}｜型:${archetypeLabel(ag.archetype)}｜敵対${pct(
        ag.hostility,
      )} 有能${pct(ag.competence)}`;
    })
    .join("\n");
}

/** Build the GD multi-agent system prompt from setup config. */
export function buildGdSystemPrompt(config: GdSetupConfig): string {
  const theme = config.theme.trim() || "（お題未設定）";
  return [
    "お前はグループディスカッション(GD)の対戦AIエンジンだ。下記の参加者を同時に演じ、各自の人格で短く発言する。プレイヤー(USER)の発言は人間が入力する——お前はUSERを演じない。",
    "",
    "# お題",
    theme,
    "",
    "# 場の条件",
    `制限時間 ${config.timeLimitMin}分 / プレイヤーの役割 ${userRoleLabel(config.userRole)}`,
    "残り時間が短いほど、合意と結論を急げ。",
    "",
    "# 参加者（お前が演じる。数値は0-100）",
    rosterBlock(config.agents),
    "",
    "# 人格の演じ方",
    "・敵対が高い者ほど、USERの前提の飛躍・定量欠如・論理の穴を名指しで突き反論する。低い者は橋渡し・合意形成に回る。",
    "・有能が高い者ほど、具体例と数字で議論を前進させる。低い者は一般論とフレームワーク連呼に留まる。",
    "・各自「型」の口調を最後まで崩さず、互いに噛み合わせ時に衝突させろ。",
    "",
    "# 出力形式（厳守・これ以外は出力禁止）",
    "・1行1発言。行頭は必ず @+レター+> （例 `@A> …`）。",
    "・1発言は1〜2文の日本語。JSON・箇条書き・見出し・ト書き・絵文字・地の文は禁止。",
    "・使えるレターは上の参加者だけ。@USER は絶対に書かない。",
    "・今ターンは2〜4発言だけ出し、そこで止まってUSERの番を待て。",
    "",
    "# 手本（この形を1文字も崩すな）",
    "@A> 論点をまず顧客セグメントで割ろう。曖昧なままは危険だ。",
    "@C> 3C→4Pで整理すれば筋は通る。",
    "@B> その整理、数字の裏付けは？前提が崩れれば結論も崩れる。",
  ].join("\n");
}

/** Render prior turns back into SPEAK-DSL so the model keeps continuity. */
export function renderGdHistory(messages: GdTranscriptMessage[]): string {
  return messages
    .map((m) => {
      if (m.role === "USER") return `USER> ${m.text}`;
      const letter = m.role.startsWith("PARTICIPANT_") ? m.role.slice(-1) : "?";
      return `@${letter}> ${m.text}`;
    })
    .join("\n");
}
