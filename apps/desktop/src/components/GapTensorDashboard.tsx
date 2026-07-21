import { useEffect, useReducer, useState } from "react";

import { TensorRadarChart } from "./TensorRadarChart";
import {
  parseGapPayload,
  sufficiencyLabel,
} from "../lib/gapPayloadView";
import {
  gapTensorDashboardReducer,
  initialGapTensorDashboardState,
} from "../lib/gapTensorDashboardReducer";
import { loadGapDaysFromRecords } from "../lib/loadGapDaysFromRecords";
import {
  calculateGapAnalysis,
  ensureAuthoritativeTensorProfile,
  getLatestGapAnalysis,
  getLatestTensorProfile,
  getTwinIdentifyStatus,
} from "../lib/pocketBrain";
import type { TwinIdentifyStatus } from "../lib/pocketBrain/types";
import { PB_UI_BUSY, PB_UI_FAIL } from "../lib/pocketBrain/uiFailure";
import {
  isPocketTensorProfile,
  pocketTensorDimensionRows,
  pocketTensorToRadarData,
} from "../lib/pocketBrainTensorView";

function fmtUnix(ts: number): string {
  if (!ts) return "—";
  const d = new Date(ts * 1000);
  if (Number.isNaN(d.getTime())) return "—";
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function ScoreMeter({ score }: { score: number }) {
  const pct = Math.max(0, Math.min(100, Math.round(score * 100)));
  return (
    <div className="gap-score-meter" aria-hidden>
      <div className="gap-score-meter-fill" style={{ width: `${pct}%` }} />
    </div>
  );
}

/**
 * M14 Gap + 6D Tensor dashboard (M18-C / M20-L).
 * Recalc pulls RECORD automatically — no manual evidence composer.
 */
export function GapTensorDashboard() {
  const [state, dispatch] = useReducer(
    gapTensorDashboardReducer,
    undefined,
    initialGapTensorDashboardState,
  );
  const [twinIdentify, setTwinIdentify] = useState<TwinIdentifyStatus | null>(
    null,
  );

  const busy = state.phase !== "idle";

  async function loadLatest() {
    dispatch({ type: "load_begin" });
    try {
      const [tensor, gap, identify] = await Promise.all([
        getLatestTensorProfile(),
        getLatestGapAnalysis(),
        getTwinIdentifyStatus().catch(() => null),
      ]);
      setTwinIdentify(identify);
      dispatch({ type: "load_success", tensor, gap });
    } catch {
      dispatch({ type: "load_failure", message: PB_UI_FAIL.gapTensorLoad });
    }
  }

  useEffect(() => {
    void loadLatest();
  }, []);

  async function onRecalculate() {
    if (busy) return;
    dispatch({ type: "recalc_begin" });
    try {
      const days = await loadGapDaysFromRecords();
      if (days.length === 0) {
        dispatch({
          type: "recalc_failure",
          message:
            "再計算できる記録がありません。RECORD に日記・予定・支出を保存してから再実行してください。",
        });
        return;
      }
      const result = await calculateGapAnalysis(days);
      const [tensor, gap] = await Promise.all([
        getLatestTensorProfile(),
        getLatestGapAnalysis(),
      ]);
      dispatch({
        type: "recalc_success",
        result,
        tensor,
        gap: gap ?? {
          id: result.id,
          created_at: 0,
          data_sufficiency: result.data_sufficiency,
          payload: result.payload,
        },
        dayCount: days.length,
      });
    } catch {
      dispatch({ type: "recalc_failure", message: PB_UI_FAIL.gapRecalc });
    }
  }

  async function onEnsureTensor() {
    if (busy) return;
    dispatch({ type: "ensure_begin" });
    try {
      const ensured = await ensureAuthoritativeTensorProfile();
      const tensor = isPocketTensorProfile(ensured)
        ? ensured
        : await getLatestTensorProfile();
      dispatch({ type: "ensure_success", tensor });
    } catch {
      dispatch({ type: "ensure_failure", message: PB_UI_FAIL.tensorEnsure });
    }
  }

  const gapView = state.gap ? parseGapPayload(state.gap.payload) : null;
  const sufficiency =
    state.lastCalculate?.data_sufficiency ??
    state.gap?.data_sufficiency ??
    gapView?.dataSufficiency ??
    null;
  const gapCount =
    state.lastCalculate?.gap_count ?? gapView?.gaps.length ?? null;

  return (
    <div className="gap-tensor-dashboard">
      <div className="profile-topline">
        <div>
          <p className="term-header">
            <span className="desktop-only">GAP_TENSOR_DASHBOARD (M14 / M18-C)</span>
            <span className="mobile-only">ギャップ分析</span>
          </p>
          <p className="hint mobile-only">
            日記と行動記録のずれを可視化します。再計算は記録データから自動で行います。
          </p>
          <p className="hint dev-noise desktop-only">
            Vault の最新 Gap 分析と 6D テンソルを表示。権威テンソルは N/A 固定（LLM は権威を更新しない）。
            再計算は RECORD 蓄積データから決定論アルゴリズムのみ（LLM 非呼び出し）。
          </p>
          {twinIdentify ? (
            <p
              className={
                twinIdentify.is_personalized
                  ? "twin-identify-badge twin-identify-fitted"
                  : "twin-identify-badge twin-identify-generic"
              }
              role="status"
              title={
                twinIdentify.is_personalized
                  ? `RLS n=${twinIdentify.n_obs} conf=${twinIdentify.confidence.toFixed(3)}`
                  : `Generic prior · n=${twinIdentify.n_obs}`
              }
            >
              {twinIdentify.is_personalized
                ? "Fitted to You"
                : "Generic Prior"}
            </p>
          ) : null}
        </div>
        <div className="action-row">
          <button
            type="button"
            className="ghost"
            disabled={busy}
            onClick={() => void loadLatest()}
          >
            {state.phase === "loading" ? "読込中…" : "最新を再読込"}
          </button>
          <button
            type="button"
            className="ghost"
            disabled={busy}
            onClick={() => void onEnsureTensor()}
          >
            {state.phase === "ensuring_tensor" ? (
              "確定中…"
            ) : (
              <>
                <span className="desktop-only">権威テンソルを確定</span>
                <span className="mobile-only">バランス分析を確定</span>
              </>
            )}
          </button>
        </div>
      </div>

      {state.phase === "loading" && (
        <p className="hint ambient-spinner" role="status">
          {PB_UI_BUSY.gapTensorLoad}
        </p>
      )}

      <div className="gap-tensor-grid">
        <div className="term-panel gap-tensor-col">
          <p className="term-header">
            <span className="desktop-only">TENSOR_PROFILE_6D</span>
            <span className="mobile-only">6次元バランス分析</span>
          </p>
          {state.tensor ? (
            <>
              <div className="term-row dev-noise">
                <span className="term-source-name">schema</span>
                <span className="term-value">{state.tensor.schema}</span>
              </div>
              <div className="term-row dev-noise">
                <span className="term-source-name">model_hash</span>
                <span className="term-value">{state.tensor.model_hash}</span>
              </div>
              <TensorRadarChart
                data={pocketTensorToRadarData(state.tensor)}
                title="Authoritative six-dimensional tensor"
              />
              <ul className="tensor-profile-summary-list">
                {pocketTensorDimensionRows(state.tensor).map((row) => (
                  <li key={row.id} className="tensor-profile-summary-row">
                    <span className="term-source-name">{row.label}</span>
                    <span className="term-value">
                      {row.score === null
                        ? "N/A / 測定不足"
                        : row.score.toFixed(2)}
                    </span>
                    <span className="term-value tensor-profile-confidence">
                      confidence {row.confidence.toFixed(2)}
                    </span>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="hint">
              <span className="desktop-only">テンソル未取得</span>
              <span className="mobile-only">バランス分析データがまだありません</span>
            </p>
          )}
        </div>

        <div className="term-panel gap-tensor-col">
          <p className="term-header">
            <span className="desktop-only">GAP_ANALYSIS</span>
            <span className="mobile-only">主観×客観ギャップ分析</span>
          </p>
          {sufficiency !== null && (
            <div className="gap-sufficiency-block">
              <div className="term-row">
                <span className="term-source-name">data_sufficiency</span>
                <span className="term-value">
                  {sufficiency.toFixed(3)} ({sufficiencyLabel(sufficiency)})
                </span>
              </div>
              <ScoreMeter score={sufficiency} />
              {gapCount !== null && (
                <div className="term-row">
                  <span className="term-source-name">gap_count</span>
                  <span className="term-value">{gapCount}</span>
                </div>
              )}
              {state.gap && (
                <>
                  <div className="term-row">
                    <span className="term-source-name">id</span>
                    <span className="term-value">{state.gap.id}</span>
                  </div>
                  <div className="term-row">
                    <span className="term-source-name">created</span>
                    <span className="term-value">
                      {fmtUnix(state.gap.created_at)}
                    </span>
                  </div>
                </>
              )}
            </div>
          )}

          {!state.gap && state.phase === "idle" && (
            <p className="hint">
              保存済み Gap がありません。「最新データで再計算」を押すと RECORD
              の蓄積から自動計算します。
            </p>
          )}

          {gapView && gapView.gaps.length > 0 && (
            <ul className="gap-flag-list">
              {gapView.gaps.map((g, i) => (
                <li key={`${g.type}-${g.theme ?? i}`} className="gap-flag-item">
                  <span className="gap-flag-type">{g.type}</span>
                  {g.theme && (
                    <span className="gap-flag-theme">{g.theme}</span>
                  )}
                  {g.gap !== null && (
                    <span className="term-value">Δ {g.gap.toFixed(3)}</span>
                  )}
                  {g.insight && <p className="hint gap-flag-insight">{g.insight}</p>}
                </li>
              ))}
            </ul>
          )}

          {gapView && (gapView.subjective.length > 0 || gapView.objective.length > 0) && (
            <div className="gap-theme-scores">
              <p className="term-header">
                <span className="desktop-only">THEME_SCORES</span>
                <span className="mobile-only">テーマ別スコア</span>
              </p>
              {gapView.subjective.map((s) => (
                <div key={`s-${s.theme}`} className="term-row mission-result-row">
                  <span className="term-source-name">主観 · {s.theme}</span>
                  <ScoreMeter score={s.score} />
                  <span className="term-value">{s.score.toFixed(2)}</span>
                </div>
              ))}
              {gapView.objective.map((o) => (
                <div key={`o-${o.theme}`} className="term-row mission-result-row">
                  <span className="term-source-name">客観 · {o.theme}</span>
                  <ScoreMeter score={o.score} />
                  <span className="term-value">{o.score.toFixed(2)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="term-panel">
        <p className="term-header">
          <span className="desktop-only">RECALCULATE</span>
          <span className="mobile-only">ギャップ再計算</span>
        </p>
        <p className="hint">
          RECORD に保存された日記・予定・支出から自動で根拠を集め、ギャップを再計算します。
        </p>
        <div className="action-row">
          <button
            type="button"
            className="primary"
            disabled={busy}
            onClick={() => void onRecalculate()}
          >
            {state.phase === "recalculating"
              ? "再計算中…"
              : "最新データで再計算"}
          </button>
          {state.phase === "recalculating" && (
            <span className="hint ambient-spinner" role="status">
              記録データを読み込み、解析しています…
            </span>
          )}
          {state.lastRecalcDayCount !== null && state.phase === "idle" && (
            <span className="hint">{state.lastRecalcDayCount} 日分を反映</span>
          )}
        </div>
      </div>

      {state.error && (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      )}
    </div>
  );
}
