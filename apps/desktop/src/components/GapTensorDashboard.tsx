import { useEffect, useReducer } from "react";

import { TensorRadarChart } from "./TensorRadarChart";
import {
  parseGapPayload,
  sufficiencyLabel,
} from "../lib/gapPayloadView";
import {
  buildDaysFromDraft,
  draftReadyForRecalc,
  gapTensorDashboardReducer,
  initialGapTensorDashboardState,
} from "../lib/gapTensorDashboardReducer";
import {
  calculateGapAnalysis,
  ensureAuthoritativeTensorProfile,
  getLatestGapAnalysis,
  getLatestTensorProfile,
  isPocketBrainInvokeError,
} from "../lib/pocketBrain";
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
 * M14 Gap + 6D Tensor dashboard (M18-C).
 * Uses existing SVG TensorRadarChart — no Recharts / external chart libs.
 */
export function GapTensorDashboard() {
  const [state, dispatch] = useReducer(
    gapTensorDashboardReducer,
    undefined,
    initialGapTensorDashboardState,
  );

  const busy = state.phase !== "idle";

  async function loadLatest() {
    dispatch({ type: "load_begin" });
    try {
      const [tensor, gap] = await Promise.all([
        getLatestTensorProfile(),
        getLatestGapAnalysis(),
      ]);
      dispatch({ type: "load_success", tensor, gap });
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `gap/tensor load: ${String(e)}`;
      dispatch({ type: "load_failure", message });
    }
  }

  useEffect(() => {
    void loadLatest();
  }, []);

  async function onRecalculate() {
    if (busy || !draftReadyForRecalc(state.draft)) return;
    dispatch({ type: "recalc_begin" });
    try {
      const days = buildDaysFromDraft(state.draft);
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
      });
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `calculate_gap_analysis: ${String(e)}`;
      dispatch({ type: "recalc_failure", message });
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
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `ensure_authoritative_tensor_profile: ${String(e)}`;
      dispatch({ type: "ensure_failure", message });
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
          <p className="term-header">GAP_TENSOR_DASHBOARD (M14 / M18-C)</p>
          <p className="hint">
            Vault の最新 Gap 分析と 6D テンソルを表示。権威テンソルは N/A 固定（LLM は権威を更新しない）。
            再計算は決定論アルゴリズムのみ（LLM 非呼び出し）。
          </p>
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
            {state.phase === "ensuring_tensor"
              ? "確定中…"
              : "権威テンソルを確定"}
          </button>
        </div>
      </div>

      {state.phase === "loading" && (
        <p className="hint ambient-spinner" role="status">
          Gap / Tensor を読込中…
        </p>
      )}

      <div className="gap-tensor-grid">
        <div className="term-panel gap-tensor-col">
          <p className="term-header">TENSOR_PROFILE_6D</p>
          {state.tensor ? (
            <>
              <div className="term-row">
                <span className="term-source-name">schema</span>
                <span className="term-value">{state.tensor.schema}</span>
              </div>
              <div className="term-row">
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
            <p className="hint">テンソル未取得</p>
          )}
        </div>

        <div className="term-panel gap-tensor-col">
          <p className="term-header">GAP_ANALYSIS</p>
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
              保存済み Gap がありません。下の根拠を入力して再計算してください。
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
              <p className="term-header">THEME_SCORES</p>
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
        <p className="term-header">RECALCULATE_EVIDENCE</p>
        <p className="hint">
          `calculate_gap_analysis` は days[] 必須。Vault から日記を自動ロードするコマンドは無いため、
          ここに主観（日記）と客観（LINE自己発話・支出・予定）を注入して再計算する。
        </p>
        <div className="term-row config-row">
          <span className="term-source-name">日付 *</span>
          <input
            value={state.draft.date}
            disabled={busy}
            onChange={(e) =>
              dispatch({ type: "patch_draft", patch: { date: e.target.value } })
            }
            placeholder="YYYY-MM-DD"
          />
        </div>
        <div className="term-row config-row">
          <span className="term-source-name">日記 (主観)</span>
          <textarea
            rows={3}
            value={state.draft.diaryText}
            disabled={busy}
            onChange={(e) =>
              dispatch({
                type: "patch_draft",
                patch: { diaryText: e.target.value },
              })
            }
            placeholder="内省・宣言テキスト"
          />
        </div>
        <div className="term-row config-row">
          <span className="term-source-name">LINE 自己発話 (客観)</span>
          <textarea
            rows={2}
            value={state.draft.lineSelfText}
            disabled={busy}
            onChange={(e) =>
              dispatch({
                type: "patch_draft",
                patch: { lineSelfText: e.target.value },
              })
            }
            placeholder="主観コーパスに混入させない（客観軸）"
          />
        </div>
        <div className="term-row config-row">
          <span className="term-source-name">支出カテゴリ / 金額</span>
          <input
            value={state.draft.expenseCategory}
            disabled={busy}
            onChange={(e) =>
              dispatch({
                type: "patch_draft",
                patch: { expenseCategory: e.target.value },
              })
            }
            placeholder="カテゴリ"
          />
          <input
            value={state.draft.expenseAmount}
            disabled={busy}
            onChange={(e) =>
              dispatch({
                type: "patch_draft",
                patch: { expenseAmount: e.target.value },
              })
            }
            placeholder="円"
            inputMode="numeric"
          />
        </div>
        <div className="term-row config-row">
          <span className="term-source-name">予定タイトル</span>
          <input
            value={state.draft.calendarTitle}
            disabled={busy}
            onChange={(e) =>
              dispatch({
                type: "patch_draft",
                patch: { calendarTitle: e.target.value },
              })
            }
            placeholder="任意"
          />
        </div>
        <div className="action-row">
          <button
            type="button"
            className="primary"
            disabled={busy || !draftReadyForRecalc(state.draft)}
            onClick={() => void onRecalculate()}
          >
            {state.phase === "recalculating"
              ? "再計算中…"
              : "最新データで Gap を再計算"}
          </button>
          {state.phase === "recalculating" && (
            <span className="hint ambient-spinner" role="status">
              決定論 Gap 解析を実行中…
            </span>
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
