import { useEffect, useRef, useState } from "react";
import {
  importLineFiles,
  loadSettings,
  syncAppleCalendar,
  syncIcsFiles,
} from "../lib/engine";

export function ImportTab() {
  const [mode, setMode] = useState<"append" | "overwrite">("append");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [appleAvailable, setAppleAvailable] = useState(false);
  const lineRef = useRef<HTMLInputElement>(null);
  const icsRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void loadSettings().then((s) => setAppleAvailable(s.apple_calendar_available));
  }, []);

  function resetInput(ref: React.RefObject<HTMLInputElement | null>) {
    if (ref.current) ref.current.value = "";
  }

  async function handleLine(files: FileList | null) {
    const list = files ? Array.from(files) : [];
    if (!list.length) return;
    setBusy(true);
    setStatus("");
    try {
      const res = await importLineFiles(list);
      const msg = res.message ?? `${list.length} 件の LINE 履歴を取り込みました`;
      setStatus(res.ok === false ? `取り込み失敗: ${msg}` : msg);
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
      resetInput(lineRef);
    }
  }

  async function handleIcs(files: FileList | null) {
    const list = files ? Array.from(files) : [];
    if (!list.length) return;
    setBusy(true);
    setStatus("");
    try {
      const res = await syncIcsFiles(list, mode);
      setStatus(
        typeof res.message === "string"
          ? res.message
          : `${list.length} 件の ICS を同期しました (${mode})`,
      );
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
      resetInput(icsRef);
    }
  }

  async function handleApple() {
    setBusy(true);
    setStatus("");
    try {
      const res = await syncAppleCalendar(mode);
      setStatus(
        typeof res.message === "string" ? res.message : "Appleカレンダーを同期しました",
      );
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel import-panel">
      <h2>データ取り込み</h2>
      <p className="hint">
        LINE エクスポート (.txt) や ICS カレンダーを取り込みます。複数ファイルを一度に選択できます。
      </p>

      <div className="mode-row">
        <span>ICS / Apple 同期モード:</span>
        <select value={mode} onChange={(e) => setMode(e.target.value as "append" | "overwrite")}>
          <option value="append">追記 (既存予定に追加)</option>
          <option value="overwrite">上書き (同一日付を置換)</option>
        </select>
      </div>

      <div className="import-block">
        <h3>LINE トーク履歴</h3>
        <p className="hint">LINE の「トーク履歴を送信」で得た .txt（複数選択可）</p>
        <input
          ref={lineRef}
          type="file"
          accept=".txt,text/plain"
          multiple
          disabled={busy}
          onChange={(e) => void handleLine(e.target.files)}
        />
      </div>

      <div className="import-block">
        <h3>ICS カレンダー</h3>
        <p className="hint">Google / Outlook などからエクスポートした .ics（複数選択可）</p>
        <input
          ref={icsRef}
          type="file"
          accept=".ics,text/calendar"
          multiple
          disabled={busy}
          onChange={(e) => void handleIcs(e.target.files)}
        />
      </div>

      <div className="import-block">
        <h3>Apple カレンダー (macOS)</h3>
        <p className="hint">
          {appleAvailable
            ? "macOS の Calendar.app が保持するローカル DB を読み取り、予定を取り込みます (外部 API 不使用)。"
            : "Apple カレンダー同期は macOS でのみ利用できます (Windows では無効)。"}
        </p>
        <button type="button" disabled={busy || !appleAvailable} onClick={() => void handleApple()}>
          Apple カレンダーを同期
        </button>
      </div>

      {status && <p className="status-line">{status}</p>}
    </section>
  );
}
