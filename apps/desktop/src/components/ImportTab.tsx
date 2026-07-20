import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  classifyDocument,
  esList,
  importDocument,
  importLineContent,
  importLineFiles,
  importStats,
  loadSettings,
  syncAppleCalendar,
  syncIcsContent,
  syncIcsFiles,
  type DocumentImportResult,
} from "../lib/engine";
import { parseEngineEvent } from "../lib/parseEngineResponse";
import type { ClassifyResult, EngineEvent, EsListItem, SourceStat } from "../lib/types";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { useCorrelationId } from "../lib/useCorrelationId";

const LOG_MAX = 50;

// F2-EXT (SPEC_FOXTROT_UI.md §2.2.2 裁定3): 確定前の分類待ちリストは揮発で
// よい (recordDraft 対象外 — 数秒で再選択できるファイル選択は「記録」では
// ない)。ただの useState でよく、モジュールシングルトンにはしない。
interface PendingItem {
  file: File;
  result: ClassifyResult;
  dest: "es" | "knowledge" | "skip";
  companyName: string;
}

interface EsConflictPending {
  fileName: string;
  content: string;
  companyName: string;
  conflict: NonNullable<DocumentImportResult["conflict"]>;
  message: string;
}

function classifyLabel(type: ClassifyResult["type"]): string {
  switch (type) {
    case "line":
      return "LINE形式 — LINE取込へ回送";
    case "ics":
      return "ICS形式 — カレンダー同期へ回送";
    case "es":
      return "ES/企画書";
    case "knowledge":
      return "知識ベース";
    case "reject":
      return "拒否";
  }
}

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
  const [esItems, setEsItems] = useState<EsListItem[]>([]);
  const [esCompanyName, setEsCompanyName] = useState("");
  const [log, setLog] = useState<string[]>(importLog);
  const [pending, setPending] = useState<PendingItem[]>([]);
  const [esConflict, setEsConflict] = useState<EsConflictPending | null>(null);
  const lineRef = useRef<HTMLInputElement>(null);
  const icsRef = useRef<HTMLInputElement>(null);
  const otherRef = useRef<HTMLInputElement>(null);
  const esRef = useRef<HTMLInputElement>(null);
  // W-29: unmount 後の setState を防ぐガード (import は unmount 後も
  // バックエンドで継続するため、importLog への push は残す — W-46)。
  const mountedRef = useRef(true);
  // SPEC_FOXTROT_UI.md §9 (Rev.10): 旧来の真偽値フラグ (W-28 裁定時導入) を
  // 撤廃し、相関ID (cid) 照合へ移行した。pkb-engine-event はコマンド非依存
  // のグローバルバスだが、自分の import が in-flight の間だけイベントを
  // 取込ログへ反映するのは cid.accepts() が構造的に保証する。
  const cid = useCorrelationId();

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

  const refreshEsList = useCallback(async () => {
    try {
      const items = await esList();
      if (mountedRef.current) setEsItems(items);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    void loadSettings().then((s) => {
      if (mountedRef.current) setAppleAvailable(s.apple_calendar_available);
    });
    void refreshStats();
    void refreshEsList();
  }, [refreshStats, refreshEsList]);

  // W-22: 解除関数を確実に return する。W-28/W-45〜W-49: 自分の in-flight
  // cid のイベントのみ処理する。
  useEffect(() => {
    const unlisten = listen<unknown>("pkb-engine-event", ({ payload: raw }) => {
      let payload: EngineEvent;
      try {
        payload = parseEngineEvent(raw);
      } catch {
        return;
      }
      if (!cid.accepts(payload)) return;
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
    if (!files?.length) return;
    setBusy(true);
    const myCid = cid.begin();
    try {
      const res = await importLineFiles(Array.from(files), myCid);
      pushImportLog(
        typeof res.message === "string" ? res.message : "LINE を取り込みました",
      );
    } catch {
      pushImportLog(uiErrorMessage("LINE_IMPORT"));
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
      }
      resetInput(lineRef);
      void refreshStats();
    }
  }

  async function handleIcs(files: FileList | null) {
    if (!files?.length) return;
    setBusy(true);
    const myCid = cid.begin();
    try {
      const res = await syncIcsFiles(Array.from(files), mode, myCid);
      pushImportLog(
        typeof res.message === "string" ? res.message : "ICS を同期しました",
      );
    } catch {
      pushImportLog(uiErrorMessage("ICS_SYNC"));
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
      }
      resetInput(icsRef);
      void refreshStats();
    }
  }

  async function handleApple() {
    setBusy(true);
    const myCid = cid.begin();
    try {
      const res = await syncAppleCalendar(mode, myCid);
      pushImportLog(
        typeof res.message === "string" ? res.message : "Apple カレンダーを同期しました",
      );
    } catch {
      pushImportLog(uiErrorMessage("APPLE_CALENDAR_SYNC"));
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
      }
      void refreshStats();
    }
  }

  async function handleOtherFiles(files: FileList | null) {
    if (!files?.length) return;
    setBusy(true);
    try {
      const next: PendingItem[] = [];
      for (const file of Array.from(files)) {
        try {
          const result = await classifyDocument(file);
          if (result.type === "es") {
            pushImportLog(
              `${file.name}: ES は下の「ES（エントリーシート）管理・取り込み」から取り込んでください`,
            );
            continue;
          }
          const dest: PendingItem["dest"] =
            result.type === "knowledge" ? "knowledge" : "skip";
          next.push({ file, result, dest, companyName: "" });
        } catch {
          pushImportLog(`${file.name}: ${uiErrorMessage("DOCUMENT_IMPORT")}`);
        }
      }
      if (mountedRef.current) {
        setPending(next);
        setLog([...importLog]);
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function handleEsFiles(files: FileList | null) {
    if (!files?.length) return;
    const company = esCompanyName.trim();
    if (!company) {
      pushImportLog("ES 取込には企業名の入力が必要です");
      if (mountedRef.current) setLog([...importLog]);
      return;
    }
    setBusy(true);
    const myCid = cid.begin();
    let gated = false;
    try {
      for (const file of Array.from(files)) {
        try {
          const classified = await classifyDocument(file);
          const content = classified.content ?? "";
          if (classified.type === "reject") {
            pushImportLog(`${file.name}: 拒否 — ${classified.reasons.join("; ")}`);
            continue;
          }
          if (classified.type === "line" || classified.type === "ics") {
            pushImportLog(
              `${file.name}: ${classified.type.toUpperCase()} 形式です。上の専用取込を使ってください`,
            );
            continue;
          }
          const res = await importDocument(content, file.name, "es", myCid, company);
          if (res.needs_confirmation && res.conflict) {
            gated = true;
            if (mountedRef.current) {
              setEsConflict({
                fileName: file.name,
                content,
                companyName: company,
                conflict: res.conflict,
                message: res.message ?? "既存ESとの表記揺れが検出されました",
              });
            }
            pushImportLog(res.message ?? `${file.name}: 置き換え確認が必要です`);
            break;
          }
          pushImportLog(res.message ?? `${file.name} を ES として取り込みました`);
          void refreshEsList();
        } catch {
          pushImportLog(uiErrorMessage("DOCUMENT_IMPORT"));
        }
      }
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
        if (!gated) resetInput(esRef);
      }
      void refreshStats();
      void refreshEsList();
    }
  }

  function updatePendingDest(index: number, dest: PendingItem["dest"]) {
    setPending((prev) => prev.map((p, i) => (i === index ? { ...p, dest } : p)));
  }

  // F2-EXT 裁定1: 「その他」は知識ベース等のみ。ES は専用セクション。
  async function handleConfirmOther() {
    const items = pending;
    if (!items.length) return;
    setBusy(true);
    const myCid = cid.begin();
    try {
      for (const item of items) {
        const { file, result, dest } = item;
        const content = result.content ?? "";
        try {
          if (result.type === "reject") {
            pushImportLog(`${file.name}: 拒否 — ${result.reasons.join("; ")}`);
            continue;
          }
          if (result.type === "line") {
            const res = await importLineContent(content, file.name, myCid);
            pushImportLog(res.message ?? `${file.name} を LINE として取り込みました`);
            continue;
          }
          if (result.type === "ics") {
            const res = await syncIcsContent(content, mode, myCid);
            pushImportLog(
              typeof res.message === "string"
                ? res.message
                : `${file.name} を ICS として同期しました`,
            );
            continue;
          }
          if (dest === "skip" || dest === "es") {
            pushImportLog(
              dest === "es"
                ? `${file.name}: ES は専用セクションから取り込んでください`
                : `${file.name}: スキップしました`,
            );
            continue;
          }
          const res = await importDocument(content, file.name, "knowledge", myCid);
          pushImportLog(res.message ?? `${file.name} を取り込みました`);
        } catch {
          pushImportLog(uiErrorMessage("DOCUMENT_IMPORT"));
        }
      }
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
        setPending([]);
        resetInput(otherRef);
      }
      void refreshStats();
    }
  }

  async function resolveEsConflict(action: "replace" | "cancel") {
    if (!esConflict) return;
    if (action === "cancel") {
      pushImportLog(`${esConflict.fileName}: 置き換えをキャンセルしました`);
      if (mountedRef.current) {
        setEsConflict(null);
        setPending([]);
        setLog([...importLog]);
      }
      return;
    }
    const primary =
      esConflict.conflict.exact ?? esConflict.conflict.similar[0] ?? null;
    setBusy(true);
    const myCid = cid.begin();
    try {
      const res = await importDocument(
        esConflict.content,
        esConflict.fileName,
        "es",
        myCid,
        esConflict.companyName,
        {
          confirmOverwrite: true,
          replaceEsId: primary?.id,
        },
      );
      pushImportLog(res.message ?? "ES を置き換えました");
      if (mountedRef.current) {
        setEsConflict(null);
        setPending([]);
      }
      void refreshEsList();
    } catch {
      pushImportLog(uiErrorMessage("DOCUMENT_IMPORT"));
    } finally {
      cid.end(myCid);
      if (mountedRef.current) {
        setBusy(false);
        setLog([...importLog]);
      }
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
        <p className="term-header">取り込み済みのデータ</p>
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

      <div className="import-block import-es-section">
        <h3>ES（エントリーシート）管理・取り込み</h3>
        <p className="hint">
          企業名を付けて ES を登録します。表記が近い既存 ES がある場合は、置き換え確認が表示されます。
        </p>

        <div className="term-panel import-es-library">
          <p className="term-header">登録済みのES（企業別）</p>
          {esItems.length > 0 ? (
            esItems.map((item) => (
              <div key={item.id} className="term-row">
                <span className="term-glyph-ok">●</span>
                <span className="term-source-name">{item.company_name}</span>
                <span className="term-value">{item.target_domain || "—"}</span>
                <span className="term-value">{item.char_count.toLocaleString()}文字</span>
                <span className="term-value">{formatMtime(item.mtime)}</span>
              </div>
            ))
          ) : (
            <p className="hint">登録済みのESはありません。</p>
          )}
        </div>

        <label className="import-es-company-label" htmlFor="import-es-company">
          企業名
        </label>
        <input
          id="import-es-company"
          type="text"
          className="import-company-input import-es-company-field"
          placeholder="例: 株式会社アルファ"
          value={esCompanyName}
          onChange={(e) => setEsCompanyName(e.target.value)}
          maxLength={64}
          disabled={busy}
        />
        <p className="hint">ファイル（.md / .txt）</p>
        <input
          ref={esRef}
          type="file"
          accept=".txt,.md,text/plain,text/markdown"
          multiple
          disabled={busy}
          onChange={(e) => void handleEsFiles(e.target.files)}
        />

        {esConflict && (
          <div className="term-panel import-es-conflict">
            <p className="term-header">ESの置き換え確認</p>
            <p className="hint">{esConflict.message}</p>
            <div className="term-row">
              <span className="term-source-name">新しい表記</span>
              <span className="term-value">{esConflict.companyName}</span>
            </div>
            {esConflict.conflict.exact && (
              <div className="term-row">
                <span className="term-source-name">既存（同一）</span>
                <span className="term-value">{esConflict.conflict.exact.company_name}</span>
              </div>
            )}
            {esConflict.conflict.similar.map((s) => (
              <div key={s.id} className="term-row">
                <span className="term-source-name">表記揺れ候補</span>
                <span className="term-value">
                  {s.company_name}（類似度 {(s.score * 100).toFixed(0)}%）
                </span>
              </div>
            ))}
            <div className="action-row">
              <button
                type="button"
                className="primary"
                disabled={busy}
                onClick={() => void resolveEsConflict("replace")}
              >
                置き換える
              </button>
              <button
                type="button"
                className="ghost"
                disabled={busy}
                onClick={() => void resolveEsConflict("cancel")}
              >
                キャンセル
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="import-block">
        <h3>その他（知識ベース・自動判別）</h3>
        <p className="hint">
          知識メモなど ES 以外のファイルを判定します。ES は上の専用セクションから取り込んでください。
        </p>
        <input
          ref={otherRef}
          type="file"
          accept=".txt,.md,.csv,.json,.ics"
          multiple
          disabled={busy}
          onChange={(e) => void handleOtherFiles(e.target.files)}
        />
      </div>

      {pending.length > 0 && (
        <div className="term-panel">
          <p className="term-header">取込前の確認</p>
          {pending.map((item, i) => (
            <div key={i} className="term-row import-pending-row">
              <span className={item.result.type === "reject" ? "term-glyph-err" : "term-glyph-ok"}>
                {item.result.type === "reject" ? "▲" : "●"}
              </span>
              <span className="term-source-name">{item.file.name}</span>
              <span className="term-value">{classifyLabel(item.result.type)}</span>
              {item.result.type === "knowledge" && (
                <select
                  value={item.dest}
                  onChange={(e) => updatePendingDest(i, e.target.value as PendingItem["dest"])}
                >
                  <option value="knowledge">知識ベース</option>
                  <option value="skip">スキップ</option>
                </select>
              )}
            </div>
          ))}
          {pending
            .filter((p) => p.result.type === "reject")
            .map((p, i) => (
              <p key={i} className="hint">
                {p.file.name}: {p.result.reasons.join("; ")}
              </p>
            ))}
          <button
            type="button"
            className="primary"
            disabled={busy}
            onClick={() => void handleConfirmOther()}
          >
            取込を確定
          </button>
        </div>
      )}

      <div className="term-panel">
        <p className="term-header">取込ログ</p>
        {log.length === 0 ? (
          <p className="hint">(取込ログはまだありません)</p>
        ) : (
          <ul
            className="term-log-list"
            role="log"
            aria-live="polite"
            aria-relevant="additions text"
          >
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
