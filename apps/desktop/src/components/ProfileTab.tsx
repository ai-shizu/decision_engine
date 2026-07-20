import { useCallback, useEffect, useState } from "react";
import {
  oraclePayload,
  oracleReport,
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

  return (
    <section className="panel profile-panel">
      <GapTensorDashboard />

      <div className="profile-topline">
        <div>
          <h2>プロファイル分析 (PROFILE)</h2>
          <p className="hint">
            Source Code / Echo メトリクスはマウント時に無菌データのみ読み込みます。
            言語化レポート・Twin・Tensor は明示ボタンのみ。
          </p>
        </div>
        <button type="button" className="ghost" disabled={busy !== null} onClick={() => void loadSterile()}>
          {busy === "refresh" ? "更新中…" : "無菌データを再読込"}
        </button>
      </div>

      <div className="profile-grid">
        <div className="profile-section">
          <p className="term-header">SOURCE_CODE</p>
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
          <p className="term-header">ECHO_METRICS</p>
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
        <p className="term-header">ORACLE_REPORT</p>
        <p className="hint">7B 言語化レポート。数値表示には使わない。明示クリックでのみ生成。</p>
        <button
          type="button"
          className="primary"
          disabled={busy !== null}
          onClick={() => void handleOracleReport()}
        >
          {busy === "oracle" ? "生成中…" : "Oracle 言語化レポートを生成"}
        </button>
        {oracleAnalysis && (
          <pre className="profile-report">{oracleAnalysis}</pre>
        )}
      </div>

      <div className="profile-section">
        <p className="term-header">TWIN_FORECAST</p>
        <div className="profile-form-row">
          <label>
            horizon_days
            <input
              type="number"
              min={1}
              max={60}
              value={horizonDays}
              onChange={(e) => setHorizonDays(Number(e.target.value))}
            />
          </label>
          <label>
            mode
            <select value={twinMode} onChange={(e) => setTwinMode(e.target.value as "daily" | "interview")}>
              <option value="daily">daily</option>
              <option value="interview">interview</option>
            </select>
          </label>
          <label>
            interview_turns
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
          {busy === "twin" ? "計算中…" : "Twin 予測を実行"}
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
        <p className="term-header">TENSOR_DIAGNOSTICS</p>
        <p className="hint">結合テンソルの再構築。明示ボタンのみ。</p>
        <button
          type="button"
          disabled={busy !== null}
          onClick={() => void handleTensorRebuild()}
        >
          {busy === "tensor" ? "再構築中…" : "Tensor を再構築"}
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
        <p className="term-header">CONTEXT_OBSERVATORY</p>
        <p className="hint">
          直近の相談で 12,000 字コンテキストが何を採用・棄却したかの決定論的マニフェスト。
          明示ボタンでのみ取得し、生本文・実名・quote は表示しません。
        </p>
        <ContextObservatoryContainer />
      </div>

      {error && (
        <p className="status-line error-text" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
