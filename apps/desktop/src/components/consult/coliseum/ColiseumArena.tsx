/**
 * Phase 14 — Arena: transcript stream + circuit breaker + I-22 AsymmetryProbe.
 * GD mode uses multi-agent mock transcript derived from setup config.
 */

import { useCallback, useMemo, useState } from "react";

import type { GdSetupConfig } from "../../../lib/gdSetupState";
import { RagChatInput } from "../../rag/RagChatInput";
import { AsymmetryProbe } from "./AsymmetryProbe";
import { CircuitBreakerGauge } from "./CircuitBreakerGauge";
import {
  TranscriptStream,
  type TranscriptMessage,
  type TranscriptRoleGd,
} from "./TranscriptStream";

/** GD live-session composer wiring (useGdSession). Omit to keep the static mock. */
export type ColiseumArenaComposer = {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  streaming: boolean;
  error?: string | null;
};

const MOCK_1ON1: TranscriptMessage[] = [
  {
    turnId: "t-0",
    role: "INTERVIEWER",
    stage: "FOUNDATION",
    text: "まず論点を MECE に分割してください。",
  },
  {
    turnId: "t-1",
    role: "CANDIDATE",
    stage: "FOUNDATION",
    text: "需要・供給・規制の三層で切ります。",
  },
  {
    turnId: "t-2",
    role: "INTERVIEWER",
    stage: "PRESSURE",
    text: "定量: オーダーは何度ですか。感度を示せ。",
  },
  {
    turnId: "t-3",
    role: "CANDIDATE",
    stage: "PRESSURE",
    text: "10^7 規模。感度は価格弾性に依存。",
  },
  {
    turnId: "t-4",
    role: "INTERVIEWER",
    stage: "PRESSURE",
    text: "その前提が崩れたとき、結論はどう変わる？",
  },
  {
    turnId: "t-5",
    role: "CANDIDATE",
    stage: "PRESSURE",
    text: "需要側が半減すればオーダーは一桁下がる。",
  },
];

const PARTICIPANT_ROLES: TranscriptRoleGd[] = [
  "PARTICIPANT_A",
  "PARTICIPANT_B",
  "PARTICIPANT_C",
  "PARTICIPANT_D",
  "PARTICIPANT_E",
];

function buildGdMockMessages(config: GdSetupConfig): TranscriptMessage[] {
  const theme = config.theme.trim() || "（お題未設定）";
  const a = PARTICIPANT_ROLES[0];
  const b = PARTICIPANT_ROLES[1] ?? PARTICIPANT_ROLES[0];
  const c = PARTICIPANT_ROLES[2] ?? PARTICIPANT_ROLES[0];
  return [
    {
      turnId: "t-0",
      role: a,
      stage: "DISCUSSION",
      text: `お題「${theme}」について、まず市場をセグメント分割すべきだ。`,
    },
    {
      turnId: "t-1",
      role: b,
      stage: "DISCUSSION",
      text: "同意。ただし顧客獲得コストを無視した議論は無意味だ。",
    },
    {
      turnId: "t-2",
      role: "USER",
      stage: "DISCUSSION",
      text: "既存顧客の LTV 改善を先に置く案はどうか。",
    },
    {
      turnId: "t-3",
      role: c,
      stage: "DISCUSSION",
      text: "フレームワークで整理すると、3C→4P が標準手順だ。",
    },
    {
      turnId: "t-4",
      role: a,
      stage: "PRESSURE",
      text: "その案の定量根拠は？感度を示せ。",
    },
    {
      turnId: "t-5",
      role: "USER",
      stage: "PRESSURE",
      text: "リピート率が 5pt 上がれば売上は約 1.3 倍。",
    },
  ];
}

/** Mock AbstractTacticSet ids (Finding 1 compile output — no vault fossils). */
const MOCK_ACTIVE_TACTICS = [
  "probe_overgeneralization",
  "stress_quant_backing",
  "force_nuanced_tradeoff",
  "probe_reproducibility",
] as const;

export function ColiseumArena({
  onRequestDebrief,
  activeTactics = [...MOCK_ACTIVE_TACTICS],
  messages,
  mode = "interview",
  gdConfig,
  composer,
}: {
  onRequestDebrief?: () => void;
  /** Override compiled tactics for live sessions later. */
  activeTactics?: string[];
  messages?: TranscriptMessage[];
  mode?: "interview" | "gd";
  gdConfig?: GdSetupConfig;
  /** GD live-session input box (useGdSession). Absent = read-only mock view. */
  composer?: ColiseumArenaComposer;
}) {
  const [tripped, setTripped] = useState(false);
  const multiAgent = mode === "gd";

  const resolvedMessages = useMemo(() => {
    if (messages) return messages;
    if (multiAgent && gdConfig) return buildGdMockMessages(gdConfig);
    return MOCK_1ON1;
  }, [messages, multiAgent, gdConfig]);

  const handleTripped = useCallback(() => {
    setTripped(true);
  }, []);

  return (
    <section className="coliseum-panel coliseum-arena" aria-label="Coliseum arena">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">
          {multiAgent ? "VIEW/ARENA · GD" : "VIEW/ARENA"}
        </span>
        <span
          className={
            tripped
              ? "coliseum-panel-tag coliseum-tag-err"
              : "coliseum-panel-tag coliseum-tag-ok"
          }
        >
          {tripped
            ? "[ CIRCUIT TRIPPED ]"
            : multiAgent
              ? "[ MULTI-AGENT STREAM ]"
              : "[ PRESSURE STREAM ]"}
        </span>
      </header>

      <CircuitBreakerGauge onTripped={handleTripped} />

      <div className="coliseum-grid-2">
        <div className="coliseum-frame coliseum-frame-tall coliseum-frame-tx">
          <TranscriptStream messages={resolvedMessages} multiAgent={multiAgent} />
          {composer && (
            <>
              <RagChatInput
                value={composer.value}
                onChange={composer.onChange}
                onSend={composer.onSend}
                streaming={composer.streaming}
                modelReady={true}
                placeholder={{ ready: "GDへ発言…", notReady: "モデル準備中…" }}
              />
              {composer.error && (
                <p className="status-line error-text" role="alert">
                  {composer.error}
                </p>
              )}
            </>
          )}
          <div className="coliseum-arena-actions">
            <button
              type="button"
              className="coliseum-btn-cyan"
              onClick={onRequestDebrief}
              disabled={tripped}
            >
              {tripped ? "FORCED → DEBRIEF (TRIPPED)" : "ADVANCE → DEBRIEF"}
            </button>
            {tripped && (
              <button
                type="button"
                className="coliseum-btn-cyan"
                onClick={onRequestDebrief}
              >
                ENTER DEBRIEF NOW
              </button>
            )}
          </div>
        </div>

        <div className="coliseum-frame coliseum-frame-tall coliseum-frame-asym">
          <AsymmetryProbe activeTactics={activeTactics} />
        </div>
      </div>
    </section>
  );
}
