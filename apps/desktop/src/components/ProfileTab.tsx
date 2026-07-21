import { useCallback, useEffect, useState } from "react";
import { runProfiler, sourceCode, tensorRebuild } from "../lib/engine";
import { todayIso } from "../lib/dateUtils";
import {
  evaluateDigitalTwinScenario,
  generateOraclePayload,
} from "../lib/pocketBrain";
import { hapticTwinWarning } from "../lib/haptics";
import {
  maxPLapseFromArray,
  twinNeedsWarning,
} from "../lib/foregroundRestore";
import type { SourceCodeView } from "../lib/types";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { ContextObservatoryContainer } from "./ContextObservatoryContainer";
import { CognitiveCalendar } from "./calendar/CognitiveCalendar";
import { GapTensorDashboard } from "./GapTensorDashboard";

function evidenceCount(evidence: unknown): number {
  return Array.isArray(evidence) ? evidence.length : 0;
}

function fmtNum(value: number | null | undefined, digits = 3): string {
  if (value === null || value === undefined) return "—";
  return value.toFixed(digits);
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function readNum(obj: Record<string, unknown> | null, key: string): number | null {
  if (!obj) return null;
  const v = obj[key];
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

function readBool(obj: Record<string, unknown> | null, key: string): boolean | null {
  if (!obj) return null;
  const v = obj[key];
  return typeof v === "boolean" ? v : null;
}

function readStr(obj: Record<string, unknown> | null, key: string): string | null {
  if (!obj) return null;
  const v = obj[key];
  return typeof v === "string" ? v : null;
}

interface OracleView {
  sufficiency: Record<string, unknown> | null;
  state: Record<string, unknown> | null;
  couplings: Record<string, unknown>[];
  forecast: Record<string, unknown> | null;
  findings: Record<string, unknown>[];
  interventions: Record<string, unknown>[];
}

function parseOracleView(payload: Record<string, unknown>): OracleView {
  const sufficiency = asRecord(payload.sufficiency);
  const state = asRecord(payload.state);
  const forecast = asRecord(payload.forecast);
  const couplingsRaw = payload.couplings;
  const findingsRaw = payload.findings;
  const interventionsRaw = payload.interventions;
  return {
    sufficiency,
    state,
    forecast,
    couplings: Array.isArray(couplingsRaw)
      ? couplingsRaw.filter((c): c is Record<string, unknown> => asRecord(c) !== null)
      : [],
    findings: Array.isArray(findingsRaw)
      ? findingsRaw.filter((f): f is Record<string, unknown> => asRecord(f) !== null)
      : [],
    interventions: Array.isArray(interventionsRaw)
      ? interventionsRaw.filter((i): i is Record<string, unknown> => asRecord(i) !== null)
      : [],
  };
}

interface TwinView {
  gate_passed: boolean;
  critical_days: string[];
  r_q50: number[];
  max_p_lapse: number | null;
  reason?: string;
}

export function ProfileTab() {
  const [source, setSource] = useState<SourceCodeView | null>(null);
  const [oracle, setOracle] = useState<OracleView | null>(null);
  const [oracleAnalysis, setOracleAnalysis] = useState("");
  const [forecast, setForecast] = useState<TwinView | null>(null);
  const [tensorResult, setTensorResult] = useState<{ rebuilt: boolean; rows: number } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");

  const [horizonDays, setHorizonDays] = useState(14);
  const [profilerMsg, setProfilerMsg] = useState("");

  const loadSterile = useCallback(async () => {
    setBusy("refresh");
    setError("");
    try {
      const today = todayIso();
      const [sc, op] = await Promise.all([
        sourceCode(),
        generateOraclePayload({ today }),
      ]);
      setSource(sc);
      setOracle(parseOracleView(op.payload));
      if (op.languageization_prompt) {
        setOracleAnalysis(op.languageization_prompt);
      }
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
      const res = await generateOraclePayload({ today: todayIso() });
      setOracle(parseOracleView(res.payload));
      setOracleAnalysis(res.languageization_prompt);
    } catch {
      setError(uiErrorMessage("ORACLE_REPORT"));
    } finally {
      setBusy(null);
    }
  }

  async function handleTwinForecast() {
    setBusy("twin");
    setError("");
    try {
      const res = await evaluateDigitalTwinScenario({
        today: todayIso(),
        horizonDays: Math.max(1, Math.min(60, Math.round(horizonDays))),
      });
      const twin = res.twin;
      const critical_days = Array.isArray(twin.forecast?.critical_days)
        ? twin.forecast.critical_days.filter((d): d is string => typeof d === "string")
        : [];
      const max_p_lapse = maxPLapseFromArray(twin.forecast?.p_lapse);
      setForecast({
        gate_passed: Boolean(twin.params?.gate_passed),
        critical_days,
        r_q50: Array.isArray(twin.forecast?.r_q50)
          ? twin.forecast.r_q50.filter((v): v is number => typeof v === "number")
          : [],
        max_p_lapse,
      });
      if (twinNeedsWarning(max_p_lapse, critical_days.length)) {
        hapticTwinWarning();
      }
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

  const daysObserved = readNum(oracle?.sufficiency ?? null, "days_observed");
  const coverage = readNum(oracle?.sufficiency ?? null, "coverage");
  const twinBss = readNum(oracle?.sufficiency ?? null, "twin_bss");
  const gatePassed = readBool(oracle?.sufficiency ?? null, "gate_passed");
  const rNow = readNum(oracle?.state ?? null, "r_now");
  const rTrend = readNum(oracle?.state ?? null, "r_trend_7d");
  const oiiEma = readNum(oracle?.state ?? null, "oii_ema");
  const oiiStreak = readNum(oracle?.state ?? null, "oii_streak_days");
  const criticalFromOracle = (() => {
    const raw = oracle?.forecast?.critical_days;
    return Array.isArray(raw)
      ? raw.filter((d): d is string => typeof d === "string")
      : [];
  })();

  return (
    <section className="panel profile-panel">
      <CognitiveCalendar />

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
                <span className="term-value">{daysObserved ?? "—"}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.coverage</span>
                <span className="term-value">{fmtNum(coverage)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.twin_bss</span>
                <span className="term-value">{fmtNum(twinBss)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">sufficiency.gate_passed</span>
                <span className="term-value">
                  {gatePassed === null ? "—" : gatePassed ? "true" : "false"}
                </span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.r_now</span>
                <span className="term-value">{fmtNum(rNow)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.r_trend_7d</span>
                <span className="term-value">{fmtNum(rTrend)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.oii_ema</span>
                <span className="term-value">{fmtNum(oiiEma)}</span>
              </div>
              <div className="profile-metric-row">
                <span className="term-source-name">state.oii_streak_days</span>
                <span className="term-value">{oiiStreak ?? "—"}</span>
              </div>
              {oracle.couplings
                .filter((c) => c.sig === true)
                .map((c, i) => (
                  <div key={`${String(c.src)}-${String(c.dst)}-${i}`} className="profile-metric-row">
                    <span className="term-source-name">
                      coupling {String(c.src ?? "?")}→{String(c.dst ?? "?")}
                    </span>
                    <span className="term-value">
                      lag {String(c.lag ?? c.lag_days ?? "—")}d ρ {fmtNum(readNum(c, "rho"))} n{" "}
                      {String(c.n_eff ?? "—")}
                    </span>
                  </div>
                ))}
              {criticalFromOracle.length > 0 && (
                <div className="profile-metric-row">
                  <span className="term-source-name">forecast.critical_days</span>
                  <span className="term-value">{criticalFromOracle.join(", ")}</span>
                </div>
              )}
              {oracle.findings.map((f, i) => (
                <div key={`${readStr(f, "rule_id") ?? i}`} className="profile-metric-row">
                  <span className="term-source-name">finding {readStr(f, "rule_id") ?? "?"}</span>
                  <span className="term-value">severity {fmtNum(readNum(f, "severity"), 2)}</span>
                </div>
              ))}
              {oracle.interventions.map((iv, i) => (
                <div key={`${readStr(iv, "bank_id") ?? i}`} className="profile-metric-row">
                  <span className="term-source-name">intervention {readStr(iv, "bank_id") ?? "?"}</span>
                  <span className="term-value">
                    rule {readStr(iv, "trigger_rule") ?? "—"} lane {String(iv.target_lane ?? "—")}
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
          Coraxis `generate_oracle_payload` の languageization_prompt。明示クリックでのみ再生成。
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
            {forecast.critical_days.length > 0 && (
              <div className="profile-metric-row">
                <span className="term-source-name">critical_days</span>
                <span className="term-value">{forecast.critical_days.join(", ")}</span>
              </div>
            )}
            {forecast.r_q50.length > 0 && (
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
