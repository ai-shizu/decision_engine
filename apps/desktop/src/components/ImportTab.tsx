import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  importLineFiles,
  importStats,
  loadSettings,
  syncAppleCalendar,
  syncIcsFiles,
} from "../lib/engine";
import type { EngineEvent, SourceStat } from "../lib/types";

const LOG_MAX = 50;

// F2 (SPEC_FOXTROT_UI.md §2.2.1 裁定2): recordDraft と同族のファイルローカル
// シングルトン。タブをアンマウントして戻っても取込ログが消えない (アプリ
// 終了で揮発)。unmount 中に届いた完了通知もここへ積む。
let importLog: string[] = [];

function pushImportLog(line: string): void {
  importLog = [...importLog, line].slice(-LOG_MAX);
}

const SOURCE_ORDER = ["diary", "line", "calendar", "finance", "es", "knowledge"] as const;
const SOURCE_LABELS: Record<(typeof SOURCE_ORDER)[number], string> = {
  diary: "日記",
  line: "LINE",
  calendar: "カレンダー",
  finance: "家計簿",
  es: "ES/企画書",
  knowledge: "知識ベース",
};

function formatMtime(mtime: string | null | undefined): string {
  if (!mtime) return "—";
  return new Date(mtime).toLocaleString();
}

export function ImportTab() {
  const [mode, setMode] = useState<"append" | "overwrite">("append");
  const [busy, setBusy] = useState(false);
  const [appleAvailable, setAppleAvailable] = useState(false);
  const [sources, setSources] = useState<Record<string, SourceStat>>({});
  const [log, setLog] = useState<string[]>(importLog);
  const lineRef = useRef<HTMLInputElement>(null);
  const icsRef = useRef<HTMLInputElement>(null);
  // W-29: unmount 後の setState を防ぐガード。
  const mountedRef = useRef(true);
  // W-28: pkb-engine-event はコマンド非依存のグローバルバス。自分の import
  // が in-flight の間だけイベントを取込ログへ反映する。
  const importingRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshStats = useCallback(async () => {
    try {
      const s = await importStats();
      if (mountedRef.current) setSources(s);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    void loadSettings().then((s) => {
      if (mountedRef.current) setAppleAvailable(s.apple_calendar_available);
    });
    void refreshStats();
  }, [refreshStats]);

  // W-22: 解除関数を確実に return する。W-28: in-flight 中のみ処理する。
  useEffect(() => {
    const unlisten = listen<EngineEvent>("pkb-engine-event", ({ payload }) => {
      if (!importingRef.current) return;
      if (payload.event === "status" && payload.message) {
        pushImportLog(payload.message);
        if (mountedRef.current) setLog([...importLog]);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  function resetInput(ref: React.RefObject<HTMLInputElement | null>) {
    if (ref.current) ref.current.value = "";
  }

  async function handleLine(files: FileList | null) {
    const list = files ? Array.from(files) : [];
    if (!list.length) return;
    setBusy(true);
    importingRef.current = true;
    try {
      const res = await importLineFiles(list);
      const msg = res.message ?? `${list.length} 件の LINE 履歴を取り込みました`;
      pushImportLog(res.ok === false ? `取り込み失敗: ${msg}` : msg);
    } catch (err) {
      pushImportLog(String(err));
    } finally {
      importingRef.current = false;
      // W-29: 中断はしない (取込は継続済み) — フロント側の反映のみガードする。
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
        resetInput(lineRef); // W-30: 同一ファイル再選択の onChange 無発火対策
      }
      void refreshStats();
    }
  }

  async function handleIcs(files: FileList | null) {
    const list = files ? Array.from(files) : [];
    if (!list.length) return;
    setBusy(true);
    importingRef.current = true;
    try {
      const res = await syncIcsFiles(list, mode);
      pushImportLog(
        typeof res.message === "string"
          ? res.message
          : `${list.length} 件の ICS を同期しました (${mode})`,
      );
    } catch (err) {
      pushImportLog(String(err));
    } finally {
      importingRef.current = false;
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
        resetInput(icsRef);
      }
      void refreshStats();
    }
  }

  async function handleApple() {
    setBusy(true);
    importingRef.current = true;
    try {
      const res = await syncAppleCalendar(mode);
      pushImportLog(typeof res.message === "string" ? res.message : "Appleカレンダーを同期しました");
    } catch (err) {
      pushImportLog(String(err));
    } finally {
      importingRef.current = false;
      if (mountedRef.current) setBusy(false);
      void refreshStats();
    }
  }

  return (
    <section className="panel import-panel">
      <h2>データ取り込み</h2>
      <p className="hint">
        LINE エクスポート (.txt) や ICS カレンダーを取り込みます。複数ファイルを一度に選択できます。
      </p>

      <div className="term-panel">
        <p className="term-header">DATA_SOURCES</p>
        {SOURCE_ORDER.map((key) => {
          const s = sources[key];
          const glyph = s?.exists ? "●" : "○";
          const glyphClass = s?.exists ? "term-glyph-ok" : "term-glyph-muted";
          return (
            <div key={key} className="term-row">
              <span className={glyphClass}>{glyph}</span>
              <span className="term-source-name">{SOURCE_LABELS[key]}</span>
              <span className="term-value">{s?.exists ? s.count.toLocaleString() : "—"}</span>
              <span className="term-value">{formatMtime(s?.mtime)}</span>
            </div>
          );
        })}
      </div>

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

      <div className="term-panel">
        <p className="term-header">IMPORT_LOG</p>
        {log.length === 0 ? (
          <p className="hint">(取込ログはまだありません)</p>
        ) : (
          <ul className="term-log-list">
            {log.map((line, i) => (
              <li key={i} className="term-log-line">
                {line}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
