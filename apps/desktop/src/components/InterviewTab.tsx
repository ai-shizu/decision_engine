import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { consult, esList, narrativeCompile, type NarrativeCompileResult } from "../lib/engine";
import { emptyCompanyFacts } from "../lib/interviewStage";
import { parseEngineEvent } from "../lib/parseEngineResponse";
import type { CompanyFacts } from "../lib/pocketBrain/types";
import type {
  EngineEvent,
  EsListItem,
  GdPersona,
  GdSpeakerTurn,
  InterviewConfig,
  InterviewMessage,
  InterviewMode,
  InterviewReport,
} from "../lib/types";
import { redactHiddenReasoning } from "../lib/redactHiddenReasoning";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { useCorrelationId } from "../lib/useCorrelationId";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { CompanyFactsForm } from "./interview/CompanyFactsForm";
import { EsReviewPanel } from "./interview/EsReviewPanel";
import { MultistageInterviewPanel } from "./interview/MultistageInterviewPanel";
import { TensorProfilePanel } from "./TensorProfilePanel";

// F-17 (SPEC_FOXTROT_UI.md §10.3): es_review は思考速度を計測も評価もしない
// (latency 構造的皆無)。hint はモード別に単一定義し、二重定義を作らない
// (line ~319 は currentMode.hint をそのまま描画するのみ)。
//
// M18-B: multistage / es_pocket は Pocket Brain Tauri 経路（Python consult 非経由）。
type InterviewSurface = InterviewMode | "multistage" | "es_pocket";

function isLegacyInterviewMode(surface: InterviewSurface): surface is InterviewMode {
  return surface === "interview_sim" || surface === "es_review" || surface === "gd_sim";
}

const MODES: { id: InterviewSurface; label: string; shortLabel: string; hint: string }[] = [
  {
    id: "interview_sim", label: "ケース/ES面接", shortLabel: "ケース",
    hint: "ES があれば敵対的 ES 面接、無ければケース面接。"
      + "回答時間を計測し、思考速度も講評対象になります。",
  },
  {
    id: "es_review", label: "ES添削 (legacy)", shortLabel: "ES旧",
    hint: "登録済みの ES を採用責任者ペルソナで容赦なく添削。"
      + "書類単体の論理的強度のみを評価します (思考速度は評価しません)。",
  },
  {
    id: "gd_sim", label: "グループディスカッション", shortLabel: "GD",
    hint: "厄介な参加者たちとのカオス GD。"
      + "回答時間を計測し、思考速度も講評対象になります。",
  },
  {
    id: "multistage", label: "多段面接 (PB)", shortLabel: "多段",
    hint: "M17 FSM: Foundation → Pressure → Debrief → Closed。"
      + "start_multistage_interview / advance_interview_stage をストリーミング結合。",
  },
  {
    id: "es_pocket", label: "ES添削 (PB)", shortLabel: "ES",
    hint: "review_es_draft: オフライン企業ファクト注入 + RAG 経験 + 採用責任者ストリーム添削。",
  },
];

const TRAIT_PRESETS = [
  "クラッシャー",
  "フリーライダー",
  "クラウザー",
  "協調型",
  "論理的",
  "アイデア型",
];
const CUSTOM_TRAIT = "__custom__";
const MAX_PERSONAS = 9;

const DEFAULT_PERSONAS: GdPersona[] = [
  { name: "学生A", trait: "クラッシャー" },
  { name: "学生B", trait: "フリーライダー" },
  { name: "学生C", trait: "クラウザー" },
];

// F4a (SPEC_FOXTROT_UI.md §7 裁定2): プリセットIDはバックエンドの静的バンク
// (INTERVIEW_INDUSTRY_BANK 等) の ID と一致させる。ラベルの表示のみここで持ち、
// 意味解決 (ES優先・difficulty反映) はすべてバックエンド側の責務。
const INDUSTRY_PRESETS: { id: string; label: string }[] = [
  { id: "foreign_it", label: "外資系IT企業" },
  { id: "foreign_finance", label: "外資系金融 (HFT/クオンツ)" },
  { id: "consulting", label: "戦略コンサルティングファーム" },
  { id: "startup", label: "急成長スタートアップ" },
];
const GENRE_PRESETS: { id: string; label: string }[] = [
  { id: "algorithm", label: "アルゴリズム・データ構造" },
  { id: "system_design", label: "システムデザイン" },
  { id: "fermi", label: "フェルミ推定・ケース" },
  { id: "behavioral", label: "行動面接" },
];
const DIFFICULTY_OPTIONS: { id: InterviewConfig["difficulty"]; label: string }[] = [
  { id: "standard", label: "標準" },
  { id: "hard", label: "高難度" },
  { id: "extreme", label: "最難関" },
];
const STANCE_OPTIONS: { id: InterviewConfig["stance"]; label: string }[] = [
  { id: "adversarial", label: "敵対的・圧迫" },
  { id: "standard", label: "標準・穏和" },
];
const CUSTOM_CONFIG = "__custom__";
const CUSTOM_THEME_MAX_CHARS = 240;
const DEFAULT_CONFIG: InterviewConfig = {
  industry: "foreign_it",
  genre: "fermi",
  difficulty: "standard",
  stance: "adversarial",
  customTheme: "",
  esId: "",
};

// F4b (SPEC_FOXTROT_UI.md §7 裁定3): スコア 0-100 を TensionMeter と同型の
// <rect>×10 計器で表示する。アニメーションなし (計器は跳ねない)。
function scoreColor(score: number): string {
  if (score >= 70) return "var(--ok)";
  if (score >= 40) return "var(--accent)";
  return "var(--err)";
}

function ScoreBar({ score }: { score: number }) {
  const lit = Math.max(0, Math.min(10, Math.round(score / 10)));
  const color = scoreColor(score);
  return (
    <svg className="score-bar" viewBox="0 0 100 10" preserveAspectRatio="none">
      {Array.from({ length: 10 }, (_, i) => (
        <rect key={i} x={i * 10 + 1} y={0} width={8} height={10} fill={i < lit ? color : "var(--border)"} />
      ))}
    </svg>
  );
}

// 話者名 → アバター色 (決定論的ハッシュ)
const AVATAR_COLORS = [
  "#e57373", "#64b5f6", "#ffb74d", "#9575cd", "#4db6ac",
  "#f06292", "#a1887f", "#90a4ae", "#aed581",
];
function avatarColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return AVATAR_COLORS[Math.abs(h) % AVATAR_COLORS.length];
}

const GD_SPEAKER_HEADER_RE = /^\[([^\][:\n]{1,24})\]:\s*(.*)$/;

/** GD_FORMAT_V1: 行頭 [話者名]: のみを認識し、文中の [学生A] は分割しない */
export function parseGdSpeakerTurns(raw: string): GdSpeakerTurn[] {
  const normalized = raw.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const lines = normalized.split("\n");
  const turns: GdSpeakerTurn[] = [];
  let current: GdSpeakerTurn | null = null;

  for (const line of lines) {
    const m = GD_SPEAKER_HEADER_RE.exec(line);
    if (m) {
      const speaker = m[1].trim();
      if (!speaker) continue;
      if (current) turns.push(current);
      current = { speaker, text: m[2].trim() };
      continue;
    }
    if (current) {
      if (line.trim() || current.text) {
        current.text = current.text ? `${current.text}\n${line}` : line;
      }
    }
  }
  if (current) turns.push(current);

  const trimmed = raw.trim();
  if (turns.length === 0 && trimmed) {
    return [{ speaker: "GD", text: trimmed }];
  }
  return turns;
}

function GdThreadMessage({ text, streaming }: { text: string; streaming?: boolean }) {
  const turns = parseGdSpeakerTurns(text);
  return (
    <div className="gd-thread">
      {turns.map((t, i) => (
        <div key={i} className="gd-turn">
          <span
            className="gd-turn-avatar persona-avatar"
            style={{ background: avatarColor(t.speaker) }}
          >
            {t.speaker.slice(0, 1) || "?"}
          </span>
          <div className="gd-turn-body">
            <span className="gd-turn-speaker">{t.speaker}</span>
            <pre className="gd-turn-text chat-text">
              {t.text}
              {streaming && i === turns.length - 1 && <span className="chat-cursor">▌</span>}
            </pre>
          </div>
        </div>
      ))}
    </div>
  );
}

// F-20 (SPEC_FOXTROT_UI.md §10.6): セッションの三状態。"debrief" は講評後の
// 対話継続フェーズ (感想戦) — バックエンドの state["phase"]=="debrief" と対応。
type SessionPhase = "idle" | "active" | "debrief";

export function InterviewTab() {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<InterviewSurface>("interview_sim");
  const mode: InterviewMode = isLegacyInterviewMode(surface) ? surface : "interview_sim";
  const [phase, setPhase] = useState<SessionPhase>("idle");
  const [messages, setMessages] = useState<InterviewMessage[]>([]);
  const [personas, setPersonas] = useState<GdPersona[]>(DEFAULT_PERSONAS);
  const [config, setConfig] = useState<InterviewConfig>(DEFAULT_CONFIG);
  const [esLibrary, setEsLibrary] = useState<EsListItem[]>([]);
  const [report, setReport] = useState<InterviewReport | null>(null);
  const [narrativeTarget, setNarrativeTarget] = useState("");
  const [narrativeResult, setNarrativeResult] = useState<NarrativeCompileResult | null>(null);
  const [narrativeBusy, setNarrativeBusy] = useState(false);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [statusKind, setStatusKind] = useState<"info" | "error">("info");
  // M20-G: shared offline company facts (EDINET inject) for all mobile modes.
  const [sharedFacts, setSharedFacts] = useState<CompanyFacts>(() => emptyCompanyFacts());
  const logRef = useRef<HTMLDivElement>(null);
  // AI メッセージ表示完了時刻 — 次のユーザー送信までの経過が response_time_sec
  const aiShownAtRef = useRef<number | null>(null);
  // SPEC_FOXTROT_UI.md §9 (Rev.10): pkb-engine-event はコマンド非依存の
  // グローバルバス (W-28)。以前は busy 系ガードが無く、他タブの status/
  // chunk が混線し得た。相関ID (cid) 照合で自分の in-flight リクエストの
  // イベントのみ処理する (F4a/F4b の config/report ロジックには無変更)。
  const cid = useCorrelationId();
  const pocketBrainSurface =
    surface === "multistage" || surface === "es_pocket";

  // Mobile: ES旧 (es_review) abolished — single ES (es_pocket) only.
  const visibleModes = isNarrow
    ? MODES.filter((m) => m.id !== "es_review")
    : MODES;

  useEffect(() => {
    if (isNarrow && surface === "es_review") {
      setSurface("es_pocket");
    }
  }, [isNarrow, surface]);

  useEffect(() => {
    void esList()
      .then((items) => setEsLibrary(items))
      .catch(() => setEsLibrary([]));
  }, []);

  function scrollToBottom() {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
    });
  }

  useEffect(() => {
    if (pocketBrainSurface) return;
    const unlisten = listen<unknown>("pkb-engine-event", ({ payload: raw }) => {
      let payload: EngineEvent;
      try {
        payload = parseEngineEvent(raw);
      } catch {
        return;
      }
      if (!cid.accepts(payload)) return;
      if (payload.event === "status" && payload.message) {
        setStatusKind("info");
        setStatus(payload.message);
        return;
      }
      if (payload.event === "chunk" && payload.text) {
        setMessages((prev) => {
          const lastMsg = prev[prev.length - 1];
          if (!lastMsg?.streaming) return prev;
          return [...prev.slice(0, -1), { ...lastMsg, text: lastMsg.text + payload.text }];
        });
        scrollToBottom();
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [pocketBrainSurface]);

  function switchSurface(next: InterviewSurface) {
    if (busy || next === surface) return;
    // モード切替 = 新しいセッション。バックエンドの状態は「開始」で上書きされる
    setSurface(next);
    setPhase("idle");
    setMessages([]);
    setStatus("");
    setReport(null);
    aiShownAtRef.current = null;
  }

  /** 送信の共通経路。placeholderRole でストリーミング先バブルの種別を指定 */
  async function send(
    query: string,
    opts: {
      withPersonas?: boolean;
      withConfig?: boolean;
      responseTime?: number;
      placeholderRole?: "ai" | "feedback";
      userEcho?: boolean;
      /** gd_sim 議論フェーズ (START 直後など phase 未反映時) */
      gdThread?: boolean;
    } = {},
  ) {
    const role = opts.placeholderRole ?? "ai";
    const shouldRenderGdThread =
      role === "ai" && mode === "gd_sim" && (opts.gdThread ?? phase === "active");
    setBusy(true);
    setStatusKind("info");
    setStatus("");
    setMessages((prev) => [
      ...prev,
      ...(opts.userEcho
        ? [{ role: "user" as const, text: query, responseTimeSec: opts.responseTime }]
        : []),
      {
        role,
        speaker: role === "feedback" ? "講評" : undefined,
        text: "",
        streaming: true,
        ...(shouldRenderGdThread ? { renderAs: "gd_thread" as const } : {}),
      },
    ]);
    scrollToBottom();
    const myCid = cid.begin();
    try {
      const res = await consult(
        query,
        {
          mode,
          ...(opts.withPersonas ? { personas } : {}),
          ...(opts.withConfig ? { config } : {}),
          ...(opts.responseTime !== undefined ? { response_time_sec: opts.responseTime } : {}),
        },
        myCid,
      );
      // F4b (W-37): バックエンドが検証済みの構造体をそのまま受け取る。
      // UI 側で JSON.parse(LLM出力) は絶対に書かない。
      if (res.report) setReport(res.report);
      setMessages((prev) => {
        const withoutPlaceholder = prev[prev.length - 1]?.streaming
          ? prev.slice(0, -1)
          : prev;
        if (role === "feedback") {
          return [...withoutPlaceholder, { role: "feedback", speaker: "講評", text: res.answer }];
        }
        // F-20: 感想戦フェーズの AI 返信は mode に依らず「メンター」。
        // gd_thread renderer は適用しない — メンターは単一の統合された声で応答する。
        if (phase === "debrief") {
          return [...withoutPlaceholder, { role: "ai", speaker: "メンター", text: res.answer }];
        }
        if (shouldRenderGdThread) {
          return [
            ...withoutPlaceholder,
            { role: "ai", text: res.answer, renderAs: "gd_thread" },
          ];
        }
        const speaker = mode === "es_review" ? "採用責任者" : "面接官";
        return [...withoutPlaceholder, { role: "ai", speaker, text: res.answer }];
      });
      aiShownAtRef.current = Date.now();
      return true;
    } catch {
      setMessages((prev) => (prev[prev.length - 1]?.streaming ? prev.slice(0, -1) : prev));
      setStatusKind("error");
      setStatus(uiErrorMessage("INTERVIEW_RESPONSE"));
      return false;
    } finally {
      setBusy(false);
      cid.end(myCid);
      scrollToBottom();
    }
  }

  async function handleStart() {
    setMessages([]);
    setReport(null);
    aiShownAtRef.current = null;
    setPhase("active");
    const ok = await send("開始", {
      withPersonas: mode === "gd_sim",
      withConfig: mode === "interview_sim" || mode === "gd_sim",
      gdThread: mode === "gd_sim",
    });
    if (!ok) setPhase("idle");
  }

  async function handleEsReview() {
    // F-17 (SPEC_FOXTROT_UI.md §10.3): es_review は思考速度を計測も評価も
    // しない。opts.responseTime を意図的に渡さない (send() は
    // opts.responseTime === undefined の時 response_time_sec を組み立てない
    // — 将来の混入防止のため、この不在は事故ではなく設計であることを明示する)。
    const name = input.trim();
    setInput("");
    await send(name || "添削", { userEcho: false });
  }

  async function handleSend(e: React.FormEvent) {
    e.preventDefault();
    const q = input.trim();
    if (!q || busy) return;
    if (mode === "es_review") {
      void handleEsReview();
      return;
    }
    setInput("");
    // F-20: 感想戦フェーズでは思考速度評価が存在しない — responseTime を送らない。
    const responseTime =
      phase === "debrief"
        ? undefined
        : aiShownAtRef.current !== null
          ? Math.round(((Date.now() - aiShownAtRef.current) / 1000) * 10) / 10
          : undefined;
    await send(q, { responseTime, userEcho: true });
  }

  async function handleFeedback() {
    if (busy || phase !== "active") return;
    const ok = await send("講評", { placeholderRole: "feedback" });
    if (ok) {
      setPhase("debrief");
      aiShownAtRef.current = null;
    }
  }

  /** F-20: 感想戦を閉じる (バックエンドへ「終了」を送り、state=None へ戻す) */
  async function handleDebriefEnd() {
    if (busy || phase !== "debrief") return;
    const ok = await send("終了", { userEcho: false });
    if (ok) {
      setPhase("idle");
      setMessages([]);
      setReport(null);
      aiShownAtRef.current = null;
    }
  }

  function updatePersona(i: number, patch: Partial<GdPersona>) {
    setPersonas((prev) => prev.map((p, j) => (j === i ? { ...p, ...patch } : p)));
  }

  async function handleNarrativeCompile() {
    setNarrativeBusy(true);
    setNarrativeResult(null);
    try {
      const domain = narrativeTarget.trim();
      const res = await narrativeCompile(domain || undefined);
      setNarrativeResult(res);
    } catch {
      setNarrativeResult({ ok: false, reason: uiErrorMessage("NARRATIVE_COMPILE") });
    } finally {
      setNarrativeBusy(false);
    }
  }

  const showLobby = !pocketBrainSurface && mode === "gd_sim" && phase === "idle";
  // F4a: 開始前のみ表示。セッション中は条件レンダリングで unmount する
  // (F-11 — display:none 等の keep-alive 化はしない)。
  const showSessionConfig =
    !pocketBrainSurface &&
    phase === "idle" &&
    (mode === "interview_sim" || mode === "gd_sim");
  const currentMode = MODES.find((m) => m.id === surface)!;

  return (
    <section className="panel interview-panel">
      <div className="consult-header">
        <h2>
          <span className="desktop-only">面接・GD シミュレーター (INTERVIEW)</span>
          <span className="mobile-only">面接</span>
        </h2>
        {!pocketBrainSurface && phase === "active" && (
          <button
            type="button"
            className="feedback-btn"
            onClick={() => void handleFeedback()}
            disabled={busy}
          >
            講評 (Feedback)
          </button>
        )}
        {!pocketBrainSurface && phase === "debrief" && (
          <button
            type="button"
            className="feedback-btn"
            onClick={() => void handleDebriefEnd()}
            disabled={busy}
          >
            感想戦を終了
          </button>
        )}
      </div>

      <div className="sub-tabs sub-tabs-pills" role="tablist" aria-label="面接モード">
        {visibleModes.map((m) => (
          <button
            key={m.id}
            type="button"
            role="tab"
            aria-selected={surface === m.id}
            className={surface === m.id ? "active" : ""}
            onClick={() => switchSurface(m.id)}
            disabled={busy}
          >
            <span className="desktop-only">{m.label}</span>
            <span className="mobile-only">{m.shortLabel}</span>
          </button>
        ))}
      </div>
      <p className="hint dev-noise">{currentMode.hint}</p>

      {isNarrow && (
        <div className="interview-shared-context">
          <CompanyFactsForm
            facts={sharedFacts}
            onPatch={(patch) =>
              setSharedFacts((prev) => ({ ...prev, ...patch }))
            }
          />
        </div>
      )}

      {surface === "multistage" && (
        <MultistageInterviewPanel
          sharedFacts={isNarrow ? sharedFacts : undefined}
          onSharedFactsPatch={
            isNarrow
              ? (patch) => setSharedFacts((prev) => ({ ...prev, ...patch }))
              : undefined
          }
          hideEmbeddedFactsForm={isNarrow}
        />
      )}
      {surface === "es_pocket" && (
        <EsReviewPanel
          sharedFacts={isNarrow ? sharedFacts : undefined}
          onSharedFactsPatch={
            isNarrow
              ? (patch) => setSharedFacts((prev) => ({ ...prev, ...patch }))
              : undefined
          }
          hideEmbeddedFactsForm={isNarrow}
        />
      )}

      {!pocketBrainSurface && (
      <>

      {showSessionConfig && (
        <div className="term-panel">
          <p className="term-header">
            <span className="desktop-only">SESSION_CONFIG</span>
            <span className="mobile-only">設定</span>
          </p>
          {mode === "interview_sim" && (
          <>
          <div className="term-row config-row">
            <span className="term-source-name">業界</span>
            <select
              value={INDUSTRY_PRESETS.some((p) => p.id === config.industry) ? config.industry : CUSTOM_CONFIG}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  industry: e.target.value === CUSTOM_CONFIG ? "" : e.target.value,
                }))
              }
            >
              {INDUSTRY_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
              <option value={CUSTOM_CONFIG}>自由記述…</option>
            </select>
            {!INDUSTRY_PRESETS.some((p) => p.id === config.industry) && (
              <input
                value={config.industry}
                onChange={(e) => setConfig((c) => ({ ...c, industry: e.target.value }))}
                placeholder="志望業界を自由に記述"
              />
            )}
          </div>
          <div className="term-row config-row">
            <span className="term-source-name">出題ジャンル</span>
            <select
              value={GENRE_PRESETS.some((p) => p.id === config.genre) ? config.genre : CUSTOM_CONFIG}
              onChange={(e) =>
                setConfig((c) => ({
                  ...c,
                  genre: e.target.value === CUSTOM_CONFIG ? "" : e.target.value,
                }))
              }
            >
              {GENRE_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
              <option value={CUSTOM_CONFIG}>自由記述…</option>
            </select>
            {!GENRE_PRESETS.some((p) => p.id === config.genre) && (
              <input
                value={config.genre}
                onChange={(e) => setConfig((c) => ({ ...c, genre: e.target.value }))}
                placeholder="出題ジャンルを自由に記述"
              />
            )}
          </div>
          <div className="term-row config-row">
            <span className="term-source-name">難易度</span>
            <select
              value={config.difficulty}
              onChange={(e) =>
                setConfig((c) => ({ ...c, difficulty: e.target.value as InterviewConfig["difficulty"] }))
              }
            >
              {DIFFICULTY_OPTIONS.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.label}
                </option>
              ))}
            </select>
          </div>
          <div className="term-row config-row">
            <span className="term-source-name">面接スタンス</span>
            <select
              value={config.stance}
              onChange={(e) =>
                setConfig((c) => ({ ...c, stance: e.target.value as InterviewConfig["stance"] }))
              }
            >
              {STANCE_OPTIONS.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.label}
                </option>
              ))}
            </select>
          </div>
          </>
          )}
          <div className="term-row config-row">
            <span className="term-source-name">持ち込みお題 / ケース課題 (任意)</span>
            <textarea
              value={config.customTheme ?? ""}
              onChange={(e) => setConfig((c) => ({ ...c, customTheme: e.target.value }))}
              placeholder="例: 自動運転車の障害物検知システムの設計 / 東京都内の信号機の数をフェルミ推定... (空欄の場合は通常進行)"
              rows={6}
              className="custom-theme-textarea"
              maxLength={CUSTOM_THEME_MAX_CHARS}
            />
            <span className="hint">
              {(config.customTheme ?? "").length}/{CUSTOM_THEME_MAX_CHARS}
            </span>
          </div>
          {mode === "interview_sim" && (
          <>
          <div className="term-row config-row">
            <span className="term-source-name">対象企業のES</span>
            <select
              value={config.esId ?? ""}
              disabled={busy}
              onChange={(e) => setConfig((c) => ({ ...c, esId: e.target.value }))}
            >
              <option value="">ゼロベース（ESなし）</option>
              {esLibrary.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.company_name}のES
                </option>
              ))}
            </select>
          </div>
          <p className="hint">
            {config.esId
              ? "選択した企業のESのみを面接官AIの前提として使います。"
              : "ESを使わず、下の業界・ジャンル設定でゼロベースの面接にします。"}
          </p>
          <div className="action-row">
            <button type="button" className="ghost" onClick={() => setConfig(DEFAULT_CONFIG)}>
              既定値に戻す
            </button>
          </div>
          </>
          )}
        </div>
      )}

      {showLobby && (
        <div className="gd-lobby">
          <h3>GD ロビー — 参加者の設定 ({personas.length}/{MAX_PERSONAS})</h3>
          {personas.map((p, i) => {
            const isPreset = TRAIT_PRESETS.includes(p.trait);
            return (
              <div key={i} className="persona-row">
                <span className="persona-avatar" style={{ background: avatarColor(p.name) }}>
                  {p.name.slice(0, 1) || "?"}
                </span>
                <input
                  value={p.name}
                  onChange={(e) => updatePersona(i, { name: e.target.value })}
                  placeholder={`学生${String.fromCharCode(65 + i)}`}
                />
                <select
                  value={isPreset ? p.trait : CUSTOM_TRAIT}
                  onChange={(e) =>
                    updatePersona(i, {
                      trait: e.target.value === CUSTOM_TRAIT ? "" : e.target.value,
                    })
                  }
                >
                  {TRAIT_PRESETS.map((t) => (
                    <option key={t} value={t}>
                      {t}
                    </option>
                  ))}
                  <option value={CUSTOM_TRAIT}>自由記述…</option>
                </select>
                {!isPreset && (
                  <input
                    value={p.trait}
                    onChange={(e) => updatePersona(i, { trait: e.target.value })}
                    placeholder="性格・属性を自由に記述"
                  />
                )}
                <button
                  type="button"
                  className="ghost"
                  onClick={() => setPersonas((prev) => prev.filter((_, j) => j !== i))}
                  disabled={personas.length <= 1}
                  aria-label="削除"
                >
                  ✕
                </button>
              </div>
            );
          })}
          <div className="action-row">
            <button
              type="button"
              onClick={() =>
                setPersonas((prev) => [
                  ...prev,
                  { name: `学生${String.fromCharCode(65 + prev.length)}`, trait: "協調型" },
                ])
              }
              disabled={personas.length >= MAX_PERSONAS}
            >
              参加者を追加
            </button>
          </div>
        </div>
      )}

      {/* ES草案生成は ES 添削 (legacy) モード専用 — ケース/GD への侵食禁止 */}
      {phase === "idle" && surface === "es_review" && (
        <div className="term-panel narrative-panel">
          <p className="term-header">
            <span className="desktop-only">NARRATIVE_DRAFT</span>
            <span className="mobile-only">ES草案生成</span>
          </p>
          <p className="hint">
            現在のスキルと目指す姿のギャップを分析し、自己PR・ESの草案を自動生成します（※模擬面接マウント中は実行不可）
          </p>
          <div className="term-row config-row">
            <span className="term-source-name">志望領域 (任意)</span>
            <input
              value={narrativeTarget}
              onChange={(e) => setNarrativeTarget(e.target.value)}
              placeholder="空欄なら登録済み ES / 既定ドメイン"
            />
          </div>
          <button
            type="button"
            disabled={narrativeBusy || busy}
            onClick={() => void handleNarrativeCompile()}
          >
            {narrativeBusy ? "コンパイル中…" : "Narrative をコンパイル"}
          </button>
          {narrativeResult && (
            <>
              <div className="term-row">
                <span className="term-source-name">ok</span>
                <span className="term-value">{narrativeResult.ok ? "true" : "false"}</span>
              </div>
              {narrativeResult.reason && (
                <div className="term-row">
                  <span className="term-source-name">reason</span>
                  <span
                    className={`term-value${narrativeResult.ok === false ? " error-text" : ""}`}
                    role={narrativeResult.ok === false ? "alert" : undefined}
                  >
                    {narrativeResult.reason}
                  </span>
                </div>
              )}
              {narrativeResult.target_domain && (
                <div className="term-row">
                  <span className="term-source-name">target_domain</span>
                  <span className="term-value">{narrativeResult.target_domain}</span>
                </div>
              )}
              {narrativeResult.draft_path && (
                <div className="term-row">
                  <span className="term-source-name">draft_path</span>
                  <span className="term-value">{narrativeResult.draft_path}</span>
                </div>
              )}
              <div className="term-row">
                <span className="term-source-name">claims</span>
                <span className="term-value">{narrativeResult.claims?.length ?? 0}</span>
              </div>
              {narrativeResult.es_text && (
                <pre className="term-es-body">{narrativeResult.es_text}</pre>
              )}
              {narrativeResult.recruiters_eye && (
                <>
                  <p className="term-header">RECRUITERS_EYE</p>
                  <pre className="term-es-body">{narrativeResult.recruiters_eye}</pre>
                </>
              )}
            </>
          )}
        </div>
      )}

      <div className="chat-log line-chat" ref={logRef}>
        {messages.length === 0 ? (
          <p className="hint chat-empty">
            {mode === "es_review"
              ? "ES 名を入力して送信 (空欄なら最新の ES を添削)。"
              : "「開始」でセッションを始めてください。"}
          </p>
        ) : (
          messages.map((m, i) => {
            if (m.role === "user") {
              return (
                <div key={i} className="line-row user">
                  <div className="line-bubble user">
                    <pre className="chat-text">{m.text}</pre>
                    {m.responseTimeSec !== undefined && (
                      <span className="latency-tag">{m.responseTimeSec}s</span>
                    )}
                  </div>
                </div>
              );
            }
            const visibleText = redactHiddenReasoning(m.text, m.streaming);
            if (m.role === "feedback") {
              return (
                <div key={i} className="line-row feedback">
                  <div className="line-bubble feedback">
                    <span className="feedback-label">■ システム講評 — 日常の Gap と統合</span>
                    <pre className="chat-text">
                      {visibleText}
                      {m.streaming && <span className="chat-cursor">▌</span>}
                    </pre>
                  </div>
                </div>
              );
            }
            if (m.renderAs === "gd_thread") {
              return (
                <div key={i} className="line-row ai gd-thread-row">
                  <GdThreadMessage text={visibleText} streaming={m.streaming} />
                </div>
              );
            }
            const name = m.speaker || "AI";
            return (
              <div key={i} className="line-row ai">
                <span className="persona-avatar" style={{ background: avatarColor(name) }}>
                  {name.slice(0, 1)}
                </span>
                <div className="line-bubble ai">
                  <span className="speaker-name">{name}</span>
                  <pre className="chat-text">
                    {visibleText}
                    {m.streaming && <span className="chat-cursor">▌</span>}
                  </pre>
                </div>
              </div>
            );
          })
        )}
      </div>

      {report && (
        <div className="term-panel mission-result-panel">
          <p className="term-header">MISSION_RESULT</p>
          <p className="hint">AI評価候補（非測定・履歴更新に不使用）</p>
          {report.metrics.length === 0 ? (
            <p className="hint">(有効な評価軸を取得できませんでした。上の講評本文を参照してください)</p>
          ) : (
            report.metrics.map((m) => (
              <div key={m.axis} className="term-row mission-result-row">
                <span className="term-source-name">{m.axis}</span>
                <ScoreBar score={m.score} />
                <span className="term-value">{m.score}</span>
              </div>
            ))
          )}
          {report.metrics.map((m) => (
            <p key={`${m.axis}-evidence`} className="hint mission-evidence">
              {m.axis}: {m.evidence}
            </p>
          ))}
          <TensorProfilePanel tensorProfile={report.tensor_profile} />
          <div className="term-row mission-result-row">
            <span className="term-source-name">実測レイテンシ (AI評価ではなく計測値)</span>
            <span className="term-value">
              中央値 {report.latency.median_sec}s / 最大 {report.latency.max_sec}s / n={report.latency.n}
            </span>
          </div>
        </div>
      )}

      <form className="consult-form" onSubmit={(e) => void handleSend(e)}>
        {phase === "idle" && mode !== "es_review" ? (
          <div className="action-row">
            <button type="button" className="primary" onClick={() => void handleStart()} disabled={busy}>
              {busy ? "準備中…" : mode === "gd_sim" ? "この参加者で GD を開始" : "面接を開始"}
            </button>
          </div>
        ) : (
          <>
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={
                mode === "es_review"
                  ? "ES 名 (空欄で最新)"
                  : phase === "debrief"
                    ? "感想戦: 質問を入力…"
                    : "発言を入力…"
              }
              rows={2}
              disabled={busy}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void handleSend(e);
              }}
            />
            <div className="action-row">
              <button type="submit" className="primary" disabled={busy || (!input.trim() && mode !== "es_review")}>
                {busy ? "送信中…" : "送信 (Ctrl+Enter)"}
              </button>
            </div>
          </>
        )}
      </form>
      {status && (
        <p
          className={`status-line${statusKind === "error" ? " error-text" : ""}`}
          role={statusKind === "error" ? "alert" : "status"}
        >
          {status}
        </p>
      )}
      </>
      )}
    </section>
  );
}
