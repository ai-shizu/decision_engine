import { useCallback, useEffect, useRef, useState } from "react";
import {
  calendarEventDates,
  loadRecord,
  saveRecord,
} from "../lib/engine";
import { formatDateLabel, parseAmount, summarizeDay, todayIso } from "../lib/dateUtils";
import { isCommitEnter } from "../lib/keyUtils";
import {
  formatLedgerYen,
  ledgerRiskLabel,
  ledgerRiskLevel,
  ledgerRowClassName,
  ledgerTypeCode,
} from "../lib/ledgerRowView";
import { defaultTime } from "../lib/timeUtils";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import type { RecordEvent, RecordSubTab, Transaction } from "../lib/types";
import { CalendarPicker } from "./CalendarPicker";
import { TimePicker } from "./TimePicker";

// SPEC_FOXTROT_UI.md §2.1.1 (Rev.4) 裁定1: タブアンマウント中の書きかけ draft を
// ファイルローカル・シングルトンで保持する (Redux/Context ではない)。アプリ終了で
// 揮発し、ディスク内容 (baseline) が保存時から変化していた場合は破棄する。
type DraftSnapshot = { events: RecordEvent[]; transactions: Transaction[]; diary: string };
let recordDraft:
  | { date: string; subTab: RecordSubTab; baseline: DraftSnapshot; work: DraftSnapshot }
  | null = null;

function snapshotsEqual(a: DraftSnapshot, b: DraftSnapshot): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function RecordTab() {
  const [date, setDate] = useState(todayIso());
  const [subTab, setSubTab] = useState<RecordSubTab>("events");
  const [events, setEvents] = useState<RecordEvent[]>([]);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [diary, setDiary] = useState("");
  const [eventDates, setEventDates] = useState<Set<string>>(new Set());
  const [status, setStatus] = useState("");
  const [statusKind, setStatusKind] = useState<"info" | "error">("info");
  const [saveNotice, setSaveNotice] = useState<{ text: string; kind: "success" | "error" } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);

  const [eventTime, setEventTime] = useState(defaultTime);
  const [eventTitle, setEventTitle] = useState("");
  const [expCat, setExpCat] = useState("");
  const [expAmt, setExpAmt] = useState("");
  const [incCat, setIncCat] = useState("");
  const [incAmt, setIncAmt] = useState("");
  const [noticeVisible, setNoticeVisible] = useState(false);

  const diaryRef = useRef<HTMLTextAreaElement>(null);
  const noticeTimerRef = useRef<number | null>(null);
  // baseline = 直近にディスクから読んだ内容 (裁定1)。unmount 時の draft 保存で
  // 使うため、常に最新の {date, subTab, events, transactions, diary} を ref に
  // 複製しておく — useEffect(cleanup) のクロージャが古い state を掴む (stale
  // closure) のを避けるための ref ミラー (W-26 と同じ思想)。
  const baselineRef = useRef<DraftSnapshot | null>(null);
  const liveRef = useRef({ date, subTab, events, transactions, diary });
  useEffect(() => {
    liveRef.current = { date, subTab, events, transactions, diary };
  });

  const refreshMarks = useCallback(async () => {
    try {
      setEventDates(new Set(await calendarEventDates()));
    } catch {
      /* ignore */
    }
  }, []);

  const loadDay = useCallback(async (d: string) => {
    setBusy(true);
    setStatus("");
    setStatusKind("info");
    try {
      const data = await loadRecord(d);
      const diskSnapshot: DraftSnapshot = {
        events: data.events,
        transactions: data.transactions,
        diary: data.diary,
      };
      baselineRef.current = diskSnapshot;
      // 裁定1-②: 同じ日付 かつ ディスクが保存時から変化していない場合のみ
      // draft を復元する。不一致ならディスクが正で、キャッシュは破棄する。
      if (
        recordDraft &&
        recordDraft.date === d &&
        snapshotsEqual(recordDraft.baseline, diskSnapshot)
      ) {
        setEvents(recordDraft.work.events);
        setTransactions(recordDraft.work.transactions);
        setDiary(recordDraft.work.diary);
        setSubTab(recordDraft.subTab);
      } else {
        setEvents(data.events);
        setTransactions(data.transactions);
        setDiary(data.diary);
        recordDraft = null;
      }
    } catch {
      setStatusKind("error");
      setStatus(uiErrorMessage("RECORD_LOAD"));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      await refreshMarks();
      await loadDay(date);
    })();
  }, []);

  // 裁定1-①: アンマウント時に書きかけの内容を draft として退避する。
  useEffect(() => {
    return () => {
      const live = liveRef.current;
      const baseline = baselineRef.current;
      if (baseline) {
        recordDraft = {
          date: live.date,
          subTab: live.subTab,
          baseline,
          work: { events: live.events, transactions: live.transactions, diary: live.diary },
        };
      }
    };
  }, []);

  // SPEC §2.1 (F-4): diary サブタブ表示時に textarea へ autofocus。
  // subTab 切替・タブ復帰 (マウント時の draft 復元) の両方でこの effect が
  // 走るため、常に最新の subTab を反映する。
  useEffect(() => {
    if (subTab === "diary") {
      diaryRef.current?.focus();
    }
  }, [subTab]);

  // saveNotice の cleanup (W-26 系: setTimeout はアンマウント/再発火時に必ず解除)。
  useEffect(() => {
    return () => {
      if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current);
    };
  }, []);

  function showNotice(text: string, kind: "success" | "error") {
    setSaveNotice({ text, kind });
    setNoticeVisible(true);
    if (noticeTimerRef.current !== null) window.clearTimeout(noticeTimerRef.current);
    if (kind === "success") {
      noticeTimerRef.current = window.setTimeout(() => setNoticeVisible(false), 3000);
    } else {
      noticeTimerRef.current = null;
    }
  }

  async function handleDateSelect(d: string) {
    setDate(d);
    await loadDay(d);
  }

  function addEvent() {
    const title = eventTitle.trim();
    if (!title) {
      setStatusKind("info");
      setStatus("予定内容を入力してください");
      return;
    }
    const next = [...events, { time: eventTime, title }].sort((a, b) =>
      a.time.localeCompare(b.time),
    );
    setEvents(next);
    setEventTitle("");
    setStatusKind("info");
    setStatus(`${formatDateLabel(date)} ${eventTime} ${title} を追加 — Ctrl+S で保存`);
  }

  function addTx(type: "expense" | "income", category: string, amountRaw: string) {
    const cat = category.trim();
    const amount = parseAmount(amountRaw);
    if (!cat) {
      setStatusKind("info");
      setStatus("カテゴリを入力してください");
      return;
    }
    if (amount === null) {
      setStatusKind("info");
      setStatus("金額は正の整数で入力してください");
      return;
    }
    const tx: Transaction = { type, category: cat, amount };
    setTransactions((prev) => [...prev, tx]);
    const label = type === "expense" ? "支出" : "収入";
    setStatusKind("info");
    setStatus(`家計簿追加 [${label}] ${cat} ${amount.toLocaleString()}円 — Ctrl+S で保存`);
    if (type === "expense") {
      setExpCat("");
      setExpAmt("");
    } else {
      setIncCat("");
      setIncAmt("");
    }
  }

  const handleSave = useCallback(async () => {
    if (!events.length && !transactions.length && !diary.trim()) {
      showNotice("予定・家計簿・日記がすべて空です", "error");
      return;
    }
    setBusy(true);
    setSaveNotice(null);
    setNoticeVisible(false);
    try {
      const result = await saveRecord(date, events, transactions, diary);
      await refreshMarks();
      const syncMsg = result.index_rebuilt
        ? "DailyContext結晶化+ベクトル同期完了"
        : "同期不要 (最新)";
      // 裁定1-③: 保存された瞬間 draft はディスクの記録へ昇格済みなので破棄する。
      recordDraft = null;
      baselineRef.current = { events, transactions, diary };
      showNotice(`${date} を保存しました — ${syncMsg}`, "success");
      setStatus("");
      setStatusKind("info");
    } catch {
      showNotice(uiErrorMessage("RECORD_SAVE"), "error");
    } finally {
      setBusy(false);
    }
  }, [date, diary, events, refreshMarks, transactions]);

  const handleSaveRef = useRef(handleSave);
  handleSaveRef.current = handleSave;

  // N3: register once — diary/events churn must not rebind the global listener.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && !e.altKey && e.key === "s") {
        e.preventDefault();
        e.stopPropagation();
        void handleSaveRef.current();
        return;
      }
      if (e.ctrlKey && !e.altKey && (e.key === "1" || e.key === "2" || e.key === "3")) {
        e.preventDefault();
        const map: Record<string, RecordSubTab> = { "1": "events", "2": "finance", "3": "diary" };
        setSubTab(map[e.key]);
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, []);

  const summary = summarizeDay(transactions);

  return (
    <section className="panel record-panel">
      <CalendarPicker
        selected={date}
        eventDates={eventDates}
        onSelect={(d) => void handleDateSelect(d)}
      />
      <p className="date-banner">編集中: {formatDateLabel(date)}</p>

      <div className="sub-tabs">
        {(
          [
            ["events", "予定"],
            ["finance", "家計簿"],
            ["diary", "日記"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={subTab === id ? "active" : ""}
            onClick={() => setSubTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {subTab === "events" && (
        <div className="sub-panel">
          <ul className="item-list">
            {events.length === 0 ? (
              <li className="hint">(この日の予定はまだありません)</li>
            ) : (
              events.map((ev, i) => (
                <li key={i} className="record-item">
                  <span className="record-item-time">{ev.time}</span>
                  <span className="record-item-label">{ev.title}</span>
                </li>
              ))
            )}
          </ul>
          <div className="event-form">
            <TimePicker value={eventTime} onChange={setEventTime} />
            <div className="form-row">
              <input
                value={eventTitle}
                onChange={(e) => setEventTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (isCommitEnter(e) && !busy) {
                    e.preventDefault();
                    addEvent();
                  }
                }}
                placeholder="ミーティング名など"
              />
              <button type="button" onClick={addEvent}>
                追加
              </button>
            </div>
          </div>
        </div>
      )}

      {subTab === "finance" && (
        <div className="sub-panel ledger">
          <div className="ledger-summary" role="group" aria-label="日次集計">
            <div className="ledger-summary-cell">
              <span className="ledger-summary-key">INC</span>
              <span className="ledger-summary-val">
                {formatLedgerYen(summary.income)}
                <span className="ledger-yen">円</span>
              </span>
            </div>
            <div className="ledger-summary-cell">
              <span className="ledger-summary-key">EXP</span>
              <span className="ledger-summary-val">
                {formatLedgerYen(summary.expense)}
                <span className="ledger-yen">円</span>
              </span>
            </div>
            <div className="ledger-summary-cell">
              <span className="ledger-summary-key">NET</span>
              <span
                className={
                  summary.net < 0
                    ? "ledger-summary-val ledger-summary-val--neg"
                    : "ledger-summary-val"
                }
              >
                {formatLedgerYen(summary.net)}
                <span className="ledger-yen">円</span>
              </span>
            </div>
          </div>

          <div className="ledger-grid" role="table" aria-label="購買台帳">
            <div className="ledger-row ledger-row--head" role="row">
              <span className="ledger-col ledger-col-type" role="columnheader">
                TYPE
              </span>
              <span className="ledger-col ledger-col-cat" role="columnheader">
                CATEGORY
              </span>
              <span className="ledger-col ledger-col-flag" role="columnheader">
                FLAG
              </span>
              <span className="ledger-col ledger-col-amt" role="columnheader">
                AMOUNT
              </span>
            </div>
            {transactions.length === 0 ? (
              <div className="ledger-empty hint" role="row">
                (この日の取引はまだありません)
              </div>
            ) : (
              transactions.map((tx, i) => {
                const risk = ledgerRiskLevel(tx.category);
                const flag = ledgerRiskLabel(risk);
                return (
                  <div key={i} className={ledgerRowClassName(risk)} role="row">
                    <span className="ledger-col ledger-col-type" role="cell">
                      {ledgerTypeCode(tx.type)}
                    </span>
                    <span className="ledger-col ledger-col-cat" role="cell">
                      {tx.category}
                    </span>
                    <span
                      className={
                        risk === "danger"
                          ? "ledger-col ledger-col-flag ledger-flag--danger"
                          : risk === "warn"
                            ? "ledger-col ledger-col-flag ledger-flag--warn"
                            : "ledger-col ledger-col-flag"
                      }
                      role="cell"
                    >
                      {flag ?? "—"}
                    </span>
                    <span className="ledger-col ledger-col-amt" role="cell">
                      {formatLedgerYen(tx.amount)}
                      <span className="ledger-yen">円</span>
                    </span>
                  </div>
                );
              })
            )}
          </div>

          <div className="ledger-forms finance-forms">
            <div className="form-row ledger-form-row">
              <span className="field-label">EXP</span>
              <input
                value={expCat}
                onChange={(e) => setExpCat(e.target.value)}
                onKeyDown={(e) => {
                  if (isCommitEnter(e) && !busy) {
                    e.preventDefault();
                    addTx("expense", expCat, expAmt);
                  }
                }}
                placeholder="食費・交通費 / 非計画・破局…"
                aria-label="支出カテゴリ"
              />
              <input
                className="ledger-amt-input"
                value={expAmt}
                onChange={(e) => setExpAmt(e.target.value)}
                onKeyDown={(e) => {
                  if (isCommitEnter(e) && !busy) {
                    e.preventDefault();
                    addTx("expense", expCat, expAmt);
                  }
                }}
                placeholder="5000"
                inputMode="numeric"
                aria-label="支出金額"
              />
              <button type="button" onClick={() => addTx("expense", expCat, expAmt)}>
                ADD
              </button>
            </div>
            <div className="form-row ledger-form-row">
              <span className="field-label">INC</span>
              <input
                value={incCat}
                onChange={(e) => setIncCat(e.target.value)}
                onKeyDown={(e) => {
                  if (isCommitEnter(e) && !busy) {
                    e.preventDefault();
                    addTx("income", incCat, incAmt);
                  }
                }}
                placeholder="給与・副業など"
                aria-label="収入カテゴリ"
              />
              <input
                className="ledger-amt-input"
                value={incAmt}
                onChange={(e) => setIncAmt(e.target.value)}
                onKeyDown={(e) => {
                  if (isCommitEnter(e) && !busy) {
                    e.preventDefault();
                    addTx("income", incCat, incAmt);
                  }
                }}
                placeholder="300000"
                inputMode="numeric"
                aria-label="収入金額"
              />
              <button type="button" onClick={() => addTx("income", incCat, incAmt)}>
                ADD
              </button>
            </div>
          </div>
        </div>
      )}

      {subTab === "diary" && (
        <div className="sub-panel">
          <textarea
            ref={diaryRef}
            className="diary-editor"
            value={diary}
            onChange={(e) => setDiary(e.target.value)}
            rows={12}
            placeholder="今日の日記…"
          />
        </div>
      )}

      <div className="save-block">
        <button type="button" className="primary" onClick={() => void handleSave()} disabled={busy}>
          {busy ? "保存中…" : "保存 (Ctrl+S)"}
        </button>
        <p
          className={`save-notice ${saveNotice?.kind ?? ""} ${noticeVisible ? "visible" : ""}${
            saveNotice?.kind === "error" ? " error-text" : ""
          }`}
          role={saveNotice?.kind === "error" ? "alert" : undefined}
        >
          {saveNotice?.text ?? ""}
        </p>
        <span className="hint">選択中の日付を一括保存</span>
      </div>
      {status && (
        <p
          className={`status-line${statusKind === "error" ? " error-text" : ""}`}
          role={statusKind === "error" ? "alert" : undefined}
        >
          {status}
        </p>
      )}
    </section>
  );
}
