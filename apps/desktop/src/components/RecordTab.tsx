import { useCallback, useEffect, useState } from "react";
import {
  calendarEventDates,
  loadRecord,
  saveRecord,
} from "../lib/engine";
import { formatDateLabel, parseAmount, summarizeDay, todayIso } from "../lib/dateUtils";
import { defaultTime } from "../lib/timeUtils";
import type { RecordEvent, RecordSubTab, Transaction } from "../lib/types";
import { CalendarPicker } from "./CalendarPicker";
import { TimePicker } from "./TimePicker";

export function RecordTab() {
  const [date, setDate] = useState(todayIso());
  const [subTab, setSubTab] = useState<RecordSubTab>("events");
  const [events, setEvents] = useState<RecordEvent[]>([]);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [diary, setDiary] = useState("");
  const [eventDates, setEventDates] = useState<Set<string>>(new Set());
  const [status, setStatus] = useState("");
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
    try {
      const data = await loadRecord(d);
      setEvents(data.events);
      setTransactions(data.transactions);
      setDiary(data.diary);
    } catch (err) {
      setStatus(String(err));
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

  async function handleDateSelect(d: string) {
    setDate(d);
    await loadDay(d);
  }

  function addEvent() {
    const title = eventTitle.trim();
    if (!title) {
      setStatus("予定内容を入力してください");
      return;
    }
    const next = [...events, { time: eventTime, title }].sort((a, b) =>
      a.time.localeCompare(b.time),
    );
    setEvents(next);
    setEventTitle("");
    setStatus(`${formatDateLabel(date)} ${eventTime} ${title} を追加 — Ctrl+S で保存`);
  }

  function addTx(type: "expense" | "income", category: string, amountRaw: string) {
    const cat = category.trim();
    const amount = parseAmount(amountRaw);
    if (!cat) {
      setStatus("カテゴリを入力してください");
      return;
    }
    if (amount === null) {
      setStatus("金額は正の整数で入力してください");
      return;
    }
    const tx: Transaction = { type, category: cat, amount };
    setTransactions((prev) => [...prev, tx]);
    const label = type === "expense" ? "支出" : "収入";
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
      setSaveNotice({ text: "予定・家計簿・日記がすべて空です", kind: "error" });
      return;
    }
    setBusy(true);
    setSaveNotice(null);
    try {
      const result = await saveRecord(date, events, transactions, diary);
      await refreshMarks();
      const syncMsg = result.index_rebuilt
        ? "DailyContext結晶化+ベクトル同期完了"
        : "同期不要 (最新)";
      setSaveNotice({ text: `${date} を保存しました — ${syncMsg}`, kind: "success" });
      setStatus("");
    } catch (err) {
      setSaveNotice({ text: String(err), kind: "error" });
    } finally {
      setBusy(false);
    }
  }, [date, diary, events, refreshMarks, transactions]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "s") {
        e.preventDefault();
        e.stopPropagation();
        void handleSave();
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [handleSave]);

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
                <li key={i}>
                  {ev.time} {ev.title}
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
        <div className="sub-panel">
          <p className="finance-summary">
            収入: {summary.income.toLocaleString()}円 | 支出:{" "}
            {summary.expense.toLocaleString()}円 | 差引: {summary.net.toLocaleString()}円
          </p>
          <ul className="item-list">
            {transactions.length === 0 ? (
              <li className="hint">(この日の取引はまだありません)</li>
            ) : (
              transactions.map((tx, i) => (
                <li key={i}>
                  [{tx.type === "expense" ? "支出" : "収入"}] {tx.category}:{" "}
                  {tx.amount.toLocaleString()}円
                </li>
              ))
            )}
          </ul>
          <div className="finance-forms">
            <div className="form-row">
              <span className="field-label">支出</span>
              <input
                value={expCat}
                onChange={(e) => setExpCat(e.target.value)}
                placeholder="食費・交通費など"
              />
              <input
                value={expAmt}
                onChange={(e) => setExpAmt(e.target.value)}
                placeholder="5000"
              />
              <button type="button" onClick={() => addTx("expense", expCat, expAmt)}>
                追加
              </button>
            </div>
            <div className="form-row">
              <span className="field-label">収入</span>
              <input
                value={incCat}
                onChange={(e) => setIncCat(e.target.value)}
                placeholder="給与・副業など"
              />
              <input
                value={incAmt}
                onChange={(e) => setIncAmt(e.target.value)}
                placeholder="300000"
              />
              <button type="button" onClick={() => addTx("income", incCat, incAmt)}>
                追加
              </button>
            </div>
          </div>
        </div>
      )}

      {subTab === "diary" && (
        <div className="sub-panel">
          <textarea
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
        {saveNotice && (
          <p className={`save-notice ${saveNotice.kind}`}>{saveNotice.text}</p>
        )}
        <span className="hint">選択中の日付を一括保存</span>
      </div>
      {status && <p className="status-line">{status}</p>}
    </section>
  );
}
