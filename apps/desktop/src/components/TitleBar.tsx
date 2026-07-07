import { getCurrentWindow } from "@tauri-apps/api/window";

// F7 (SPEC_FOXTROT_UI.md §2.7): ブラウザプレビュー (vite 単体) では
// window.__TAURI_INTERNALS__ が存在しない。フィーチャー検出でウィンドウ
// 操作ボタンを非表示にし、ブラウザ側の検証パイプラインを壊さない。
const isTauri = "__TAURI_INTERNALS__" in window;

export function TitleBar() {
  return (
    <div className="titlebar">
      <div className="titlebar-drag" data-tauri-drag-region>
        <span className="titlebar-label">PKB</span>
      </div>
      {isTauri && (
        <div className="titlebar-controls">
          <button
            type="button"
            className="titlebar-btn"
            aria-label="最小化"
            onClick={() => void getCurrentWindow().minimize()}
          >
            &#x2500;
          </button>
          <button
            type="button"
            className="titlebar-btn"
            aria-label="最大化"
            onClick={() => void getCurrentWindow().toggleMaximize()}
          >
            &#x25a1;
          </button>
          <button
            type="button"
            className="titlebar-btn titlebar-btn-close"
            aria-label="閉じる"
            onClick={() => void getCurrentWindow().close()}
          >
            &#x2715;
          </button>
        </div>
      )}
    </div>
  );
}
