import { useCallback, useEffect, useState } from "react";
import {
  oraclePayload,
  oracleReport,
  runProfiler,
  sourceCode,
  tensorRebuild,
  twinForecast,
  type OraclePayload,
  type TwinForecast,
  type TwinScenario,
} from "../lib/engine";
import type { SourceCodeView } from "../lib/types";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { ContextObservatoryContainer } from "./ContextObservatoryContainer";
import { GapTensorDashboard } from "./GapTensorDashboard";

function evidenceCount(evidence: unknown): number {
  return Array.isArray(evidence) ? evidence.length : 0;
}

function fmtNum(value: number | null | undefined, digits = 3): string {
  if (value === null || value === undefined) return "—";
  return value.toFixed(digits);
}

export function ProfileTab() {
  const [source, setSource] = useState<SourceCodeView | null>(null);
  const [oracle, setOracle] = useState<OraclePayload | null>(null);
  const [oracleAnalysis, setOracleAnalysis] = useState("");
  const [forecast, setForecast] = useState<TwinForecast | null>(null);
  const [tensorResult, setTensorResult] = useState<{ rebuilt: boolean; rows: number } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");

  const [horizonDays, setHorizonDays] = useState(14);
  const [twinMode, setTwinMode] = useState<"daily" | "interview">("daily");
  const [interviewTurns, setInterviewTurns] = useState(0);
  const [profilerMsg, setProfilerMsg] = useState("");

  const loadSterile = useCallback(async () => {
    setBusy("refresh");
    setError("");
    try {
      const [sc, op] = await Promise.all([sourceCode(), oraclePayload("global")]);
      setSource(sc);
      setOracle(op);
    } catch {
      setError(uiErrorMessage("PROFILE_LOAD"));
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    void loadSterile();
  }, [loadSterile]);

  async function handleOracleReport() {
    setBusy("oracle");
    setError("");
    try {
      const res = await oracleReport("global");
      setOracle(res.payload);
      setOracleAnalysis(res.analysis);
    } catch {
      setError(uiErrorMessage("ORACLE_REPORT"));
    } finally {
      setBusy(null);
    }
  }

  async function handleTwinForecast() {
    const scenario: TwinScenario = {
      horizon_days: Math.max(1, Math.min(60, Math.round(horizonDays))),
      calendar: [],
      mode: twinMode,
      interview_turns: twinMode === "interview" ? Math.max(0, Math.min(20, Math.round(interviewTurns))) : null,
    };
    setBusy("twin");
    setError("");
    try {
      const res = await twinForecast(scenario, "global");
      setForecast(res);
    } catch {
      setError(uiErrorMessage("TWIN_FORECAST"));
    } finally {
      setBusy(null);
    }
  }

  async function handleTensorRebuild() {
    setBusy("tensor");
    setError("");
    try {
      const res = await tensorRebuild();
      setTensorResult(res);
    } catch {
      setError(uiErrorMessage("TENSOR_REBUILD"));
    } finally {
      setBusy(null);
    }
  }

  async function handleProfilerRebuild() {
    setBusy("profiler");
    setError("");
    setProfilerMsg("");
    try {
      const res = await runProfiler();
      setProfilerMsg(res.message);
      await loadSterile();
    } catch {
      setError(uiErrorMessage("PROFILER_RUN"));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="panel profile-panel">
      <GapTensorDashboard />

      <div className="profile-topline">
        <div>
          <h2>
            <span className="desktop-only">プロファイル分析 (PROFILE)</span>
            <span className="mobile-only">プロフィール</span>
          </h2>
          <p className="hint dev-noise">
            Source Code / Echo メトリクスはマウント時に無菌データのみ読み込みます。
            言語化レポート・Twin・Tensor は明示ボタンのみ。
          </p>
        </div>
        <button type="button" className="ghost" disabled={busy !== null} onClick={() => void loadSterile()}>
          {busy === "refresh" ? "更新中…" : (
            <>
              <span className="desktop-only">無菌データを再読込</span>
              <span className="mobile-only">データを再読込</span>
            </>
          )}
        </button>
      </div>

      <div className="profile-grid">
        <div className="profile-section">
          <p className="term-header">
            <span className="desktop-only">SOURCE_CODE</span>
            <span className="mobile-only">思考のソース（生ログ）</span>
          </p>
          {source ? (
            <div className="profile-axis-list">
              {Object.entries(source.axes).map(([axisId, axis]) => (
                <div key={axisId} className="profile-axis-row">
                  <span className="term-source-name">{axisId}</span>
                  <span className="term-value">score {fmtNum(axis.score)}</span>
                  <span className="term-value">conf {fmtNum(axis.confidence)}</span>
                  <span className="term-value">evidence {evidenceCount(axis.evidence)}</span>
                </div>
              ))}
              <div className="profile-metric-row">
                <span className="term-source-name">progress</span>
                <span className="term-value">{fmtNum(source.progress ?? 0)}</span>
              </div>
            </div>
          ) : (
            <p className="hint">(読込中または未取得)</p>
          )}
        </div>

        <div className="profile-section">
          <p className="term-header">
            <span className="desktop-only">ECHO_METRICS</span>
            <span className="mobile-only">対話・行動指標</span>
          </p>
          {oracle ? (
            <>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.days_observed</span>
                <span className="term-value">{oracle.sufficiency.days_observed}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.coverage</span>
                <span className="term-value">{fmtNum(oracle.sufficiency.coverage)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.twin_bss</span>
                <span className="term-value">{fmtNum(oracle.sufficiency.twin_bss)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.gate_passed</span>
                <span className="term-value">{oracle.sufficiency.gate_passed ? "true" : "false"}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.r_now</span>
                <span className="term-value">{fmtNum(oracle.state.r_now)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.r_trend_7d</span>
                <span className="term-value">{fmtNum(oracle.state.r_trend_7d)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.oii_ema</span>
                <span className="term-value">{fmtNum(oracle.state.oii_ema)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.oii_streak_days</span>
                <span className="term-value">{oracle.state.oii_streak_days}</span>
              </div>
              {oracle.couplings.filter((c) => c.sig).map((c, i) => (
                <div key={`${c.src}-${c.dst}-${i}`} className="profile-metric-row">
                  <span className="term-source-name">coupling {c.src}→{c.dst}</span>
                  <span className="term-value">
                    lag {c.lag_days}d ρ {fmtNum(c.rho)} n {c.n_eff}
                  </span>
                </div>
              ))}
              {oracle.forecast.critical_days.length > 0 && (
                <div className="profile-metric-row">
                  <span className="term-source-name">forecast.critical_days</span>
                  <span className="term-value">{oracle.forecast.critical_days.join(", ")}</span>
                </div>
              )}
              {oracle.findings.map((f) => (
                <div key={f.rule_id} className="profile-metric-row">
                  <span className="term-source-name">finding {f.rule_id}</span>
                  <span className="term-value">severity {fmtNum(f.severity, 2)}</span>
                </div>
              ))}
              {oracle.interventions.map((iv) => (
                <div key={iv.bank_id} className="profile-metric-row">
                  <span className="term-source-name">intervention {iv.bank_id}</span>
                  <span className="term-value">
                    rule {iv.trigger_rule} lane {iv.target_lane}
                  </span>
                </div>
              ))}
            </>
          ) : (
            <p className="hint">(読込中または未取得)</p>
          )}
        </div>
      </div>

      <div className="profile-section">
        <p className="term-header">
          <span className="desktop-only">ORACLE_REPORT</span>
          <span className="mobile-only">予測の解説レポート</span>
        </p>
        <p className="hint mobile-only">
          数値の代わりに、いまの傾向を文章で説明します。ボタンを押したときだけ生成します。
        </p>
        <p className="hint dev-noise desktop-only">
          7B 言語化レポート。数値表示には使わない。明示クリックでのみ生成。
        </p>
        <button
          type="button"
          className="primary"
          disabled={busy !== null}
          onClick={() => void handleOracleReport()}
        >
          {busy === "oracle" ? "生成中…" : (
            <>
              <span className="desktop-only">Oracle 言語化レポートを生成</span>
              <span className="mobile-only">解説レポートを生成</span>
            </>
          )}
        </button>
        {oracleAnalysis && (
          <pre className="profile-report">{oracleAnalysis}</pre>
        )}
      </div>

      <div className="profile-section">
        <p className="term-header">
          <span className="desktop-only">TWIN_FORECAST</span>
          <span className="mobile-only">将来予測シミュレーション</span>
        </p>
        <div className="profile-form-row">
          <label>
            <span className="desktop-only">horizon_days</span>
            <span className="mobile-only">予測日数</span>
            <input
              type="number"
              min={1}
              max={60}
              value={horizonDays}
              onChange={(e) => setHorizonDays(Number(e.target.value))}
            />
          </label>
          <label>
            <span className="desktop-only">mode</span>
            <span className="mobile-only">モード</span>
            <select value={twinMode} onChange={(e) => setTwinMode(e.target.value as "daily" | "interview")}>
              <option value="daily">日常</option>
              <option value="interview">面接</option>
            </select>
          </label>
          <label>
            <span className="desktop-only">interview_turns</span>
            <span className="mobile-only">面接ターン数</span>
            <input
              type="number"
              min={0}
              max={20}
              value={interviewTurns}
              disabled={twinMode !== "interview"}
              onChange={(e) => setInterviewTurns(Number(e.target.value))}
            />
          </label>
        </div>
        <button
          type="button"
          disabled={busy !== null}
          onClick={() => void handleTwinForecast()}
        >
          {busy === "twin" ? (
            "計算中…"
          ) : (
            <>
              <span className="desktop-only">Twin 予測を実行</span>
              <span className="mobile-only">将来予測を実行</span>
            </>
          )}
        </button>
        {forecast && (
          <>
            <div className="profile-metric-row">
              <span className="term-source-name">gate_passed</span>
              <span className="term-value">{forecast.gate_passed ? "true" : "false"}</span>
            </div>
            {forecast.reason && (
              <div className="profile-metric-row">
                <span className="term-source-name">reason</span>
                <span className="term-value">{forecast.reason}</span>
              </div>
            )}
            {forecast.critical_days && forecast.critical_days.length > 0 && (
              <div className="profile-metric-row">
                <span className="term-source-name">critical_days</span>
                <span className="term-value">{forecast.critical_days.join(", ")}</span>
              </div>
            )}
            {forecast.r_q50 && (
              <div className="profile-metric-row">
                <span className="term-source-name">r_q50 (head)</span>
                <span className="term-value">
                  {forecast.r_q50.slice(0, 5).map((v) => fmtNum(v)).join(", ")}
                </span>
              </div>
            )}
          </>
        )}
      </div>

      <div className="profile-section">
        <p className="term-header">
          <span className="desktop-only">TENSOR_DIAGNOSTICS</span>
          <span className="mobile-only">バランス分析の再構築</span>
        </p>
        <p className="hint desktop-only">結合テンソルの再構築。明示ボタンのみ。</p>
        <p className="hint mobile-only">バランス分析データを明示的に再構築します。</p>
        <button
          type="button"
          disabled={busy !== null}
          onClick={() => void handleTensorRebuild()}
        >
          {busy === "tensor" ? (
            "再構築中…"
          ) : (
            <>
              <span className="desktop-only">Tensor を再構築</span>
              <span className="mobile-only">分析データを再構築</span>
            </>
          )}
        </button>
        {tensorResult && (
          <div className="profile-metric-row">
            <span className="term-source-name">rebuild</span>
            <span className="term-value">
              rebuilt={tensorResult.rebuilt ? "true" : "false"} rows={tensorResult.rows}
            </span>
          </div>
        )}
      </div>

      <div className="profile-section">
        <p className="term-header">
          <span className="desktop-only">CONTEXT_OBSERVATORY</span>
          <span className="mobile-only">相談コンテキストの内訳</span>
        </p>
        <p className="hint desktop-only">
          直近の相談で 12,000 字コンテキストが何を採用・棄却したかの決定論的マニフェスト。
          明示ボタンでのみ取得し、生本文・実名・quote は表示しません。
        </p>
        <p className="hint mobile-only">
          直近の相談でどの情報を採用・見送ったかの内訳です。ボタンを押したときだけ取得します。
        </p>
        <ContextObservatoryContainer />
      </div>

      <div className="profile-section">
        <p className="term-header">AIによる自己プロフィールの再構築</p>
        <p className="hint">
          日記・予定・家計など取り込み済みのデータから、自己理解プロフィールを再計算します。
        </p>
        <button
          type="button"
          className="secondary"
          disabled={busy !== null}
          onClick={() => void handleProfilerRebuild()}
        >
          {busy === "profiler" ? "再構築中…" : "プロフィールを再構築"}
        </button>
        {profilerMsg && (
          <p className="status-line" role="status">
            {profilerMsg}
          </p>
        )}
      </div>

      {error && (
        <p className="status-line error-text" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
