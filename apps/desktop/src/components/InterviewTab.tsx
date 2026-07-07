import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { consult } from "../lib/engine";
import type { EngineEvent, GdPersona, InterviewMessage, InterviewMode } from "../lib/types";

const MODES: { id: InterviewMode; label: string; hint: string }[] = [
  { id: "interview_sim", label: "ケース/ES面接", hint: "ES があれば敵対的 ES 面接、無ければケース面接" },
  { id: "es_review", label: "ES添削", hint: "data/es/ の ES を採用責任者ペルソナで容赦なく添削" },
  { id: "gd_sim", label: "グループディスカッション", hint: "厄介な参加者たちとのカオス GD" },
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

/** GD の AI 応答を [話者名] 区切りで複数メッセージへ分解する */
function splitSpeakers(text: string): { speaker: string; text: string }[] {
  const re = /\[([^\][\n]{1,24})\]\s*/g;
  const out: { speaker: string; text: string }[] = [];
  let last: { speaker: string; index: number } | null = null;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (last) {
      const body = text.slice(last.index, m.index).trim();
      if (body) out.push({ speaker: last.speaker, text: body });
    }
    last = { speaker: m[1], index: re.lastIndex };
  }
  if (last) {
    const body = text.slice(last.index).trim();
    if (body) out.push({ speaker: last.speaker, text: body });
  }
  return out.length ? out : [{ speaker: "", text: text.trim() }];
}

export function InterviewTab() {
  const [mode, setMode] = useState<InterviewMode>("interview_sim");
  const [sessionActive, setSessionActive] = useState(false);
  const [messages, setMessages] = useState<InterviewMessage[]>([]);
  const [personas, setPersonas] = useState<GdPersona[]>(DEFAULT_PERSONAS);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const logRef = useRef<HTMLDivElement>(null);
  // AI メッセージ表示完了時刻 — 次のユーザー送信までの経過が response_time_sec
  const aiShownAtRef = useRef<number | null>(null);

  function scrollToBottom() {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
    });
  }

  useEffect(() => {
    const unlisten = listen<EngineEvent>("pkb-engine-event", ({ payload }) => {
      if (payload.event === "status" && payload.message) {
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
  }, []);

  function switchMode(next: InterviewMode) {
    if (busy || next === mode) return;
    // モード切替 = 新しいセッション。バックエンドの状態は「開始」で上書きされる
    setMode(next);
    setSessionActive(false);
    setMessages([]);
    setStatus("");
    aiShownAtRef.current = null;
  }

  /** 送信の共通経路。placeholderRole でストリーミング先バブルの種別を指定 */
  async function send(
    query: string,
    opts: {
      withPersonas?: boolean;
      responseTime?: number;
      placeholderRole?: "ai" | "feedback";
      userEcho?: boolean;
    } = {},
  ) {
    const role = opts.placeholderRole ?? "ai";
    setBusy(true);
    setStatus("");
    setMessages((prev) => [
      ...prev,
      ...(opts.userEcho
        ? [{ role: "user" as const, text: query, responseTimeSec: opts.responseTime }]
        : []),
      { role, speaker: role === "feedback" ? "講評" : undefined, text: "", streaming: true },
    ]);
    scrollToBottom();
    try {
      const res = await consult(query, {
        mode,
        ...(opts.withPersonas ? { personas } : {}),
        ...(opts.responseTime !== undefined ? { response_time_sec: opts.responseTime } : {}),
      });
      setMessages((prev) => {
        const withoutPlaceholder = prev[prev.length - 1]?.streaming
          ? prev.slice(0, -1)
          : prev;
        if (role === "feedback") {
          return [...withoutPlaceholder, { role: "feedback", speaker: "講評", text: res.answer }];
        }
        if (mode === "gd_sim") {
          return [
            ...withoutPlaceholder,
            ...splitSpeakers(res.answer).map((s) => ({
              role: "ai" as const,
              speaker: s.speaker || "参加者",
              text: s.text,
            })),
          ];
        }
        const speaker = mode === "es_review" ? "採用責任者" : "面接官";
        return [...withoutPlaceholder, { role: "ai", speaker, text: res.answer }];
      });
      aiShownAtRef.current = Date.now();
      return true;
    } catch (err) {
      setMessages((prev) => (prev[prev.length - 1]?.streaming ? prev.slice(0, -1) : prev));
      setStatus(String(err));
      return false;
    } finally {
      setBusy(false);
      scrollToBottom();
    }
  }

  async function handleStart() {
    setMessages([]);
    aiShownAtRef.current = null;
    const ok = await send("開始", { withPersonas: mode === "gd_sim" });
    if (ok) setSessionActive(true);
  }

  async function handleEsReview() {
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
    const responseTime =
      aiShownAtRef.current !== null
        ? Math.round(((Date.now() - aiShownAtRef.current) / 1000) * 10) / 10
        : undefined;
    await send(q, { responseTime, userEcho: true });
  }

  async function handleFeedback() {
    if (busy || !sessionActive) return;
    const ok = await send("講評", { placeholderRole: "feedback" });
    if (ok) {
      setSessionActive(false);
      aiShownAtRef.current = null;
    }
  }

  function updatePersona(i: number, patch: Partial<GdPersona>) {
    setPersonas((prev) => prev.map((p, j) => (j === i ? { ...p, ...patch } : p)));
  }

  const showLobby = mode === "gd_sim" && !sessionActive;
  const currentMode = MODES.find((m) => m.id === mode)!;

  return (
    <section className="panel interview-panel">
      <div className="consult-header">
        <h2>面接・GD シミュレーター (INTERVIEW)</h2>
        {sessionActive && (
          <button
            type="button"
            className="feedback-btn"
            onClick={() => void handleFeedback()}
            disabled={busy}
          >
            講評 (Feedback)
          </button>
        )}
      </div>

      <div className="sub-tabs">
        {MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            className={mode === m.id ? "active" : ""}
            onClick={() => switchMode(m.id)}
            disabled={busy}
          >
            {m.label}
          </button>
        ))}
      </div>
      <p className="hint">{currentMode.hint}。回答時間は計測され、思考速度も講評対象になります。</p>

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
            if (m.role === "feedback") {
              return (
                <div key={i} className="line-row feedback">
                  <div className="line-bubble feedback">
                    <span className="feedback-label">■ システム講評 — 日常の Gap と統合</span>
                    <pre className="chat-text">
                      {m.text}
                      {m.streaming && <span className="chat-cursor">▌</span>}
                    </pre>
                  </div>
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
                    {m.text}
                    {m.streaming && <span className="chat-cursor">▌</span>}
                  </pre>
                </div>
              </div>
            );
          })
        )}
      </div>

      <form className="consult-form" onSubmit={(e) => void handleSend(e)}>
        {!sessionActive && mode !== "es_review" ? (
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
              placeholder={mode === "es_review" ? "ES 名 (空欄で最新)" : "発言を入力…"}
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
      {status && <p className="status-line">{status}</p>}
    </section>
  );
}
