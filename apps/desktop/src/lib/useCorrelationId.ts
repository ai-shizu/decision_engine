import { useEffect, useRef } from "react";

/**
 * SPEC_FOXTROT_UI.md §9.1 (Rev.10): 各タブが個別に持っていた真偽値フラグ群
 * (F3/F2 裁定時導入) を完全撤廃する後継フック。
 * 「1関数を全員が通る」規律 (keyUtils/useThrottledStream と同列) — 全タブが
 * このフックのみを経由して in-flight リクエストとイベントの相関を取る。
 *
 * W-49 (直列性の境界の明示): このフックは「イベントを正しい宛先へ配る」
 * (混線防止) だけを解決する。「2つのリクエストを同時に飛ばす」(真の並行
 * 実行) は engine.rs::invoke_sync のプロセスロックが構造的に禁止しており、
 * このフックはそれを一切変えない。エンジン多重化 (§9.4 Target Golf) が
 * land するまで、pkbInvoke は並行前提で使うな。
 */

// W-47 (cid の一意性): アプリ全域単調カウンタのみを唯一の採番源とする。
// Math.random() (衝突可能) も Date.now() (同一ms衝突) もコンポーネント
// ごとのローカルカウンタ (コンポーネント間で衝突する) も禁止。単一の
// モジュールレベル変数のみ。セッション跨ぎでリセットされるのは無害
// (cid はセッションスコープ)。
let _cidSeq = 0;

export function useCorrelationId() {
  // 現在の in-flight cid (無ければ null)。「履歴」ではなく「現在の唯一の
  // in-flight」を持つ単一スロット — W-45: 複数同時 in-flight を1コンポー
  // ネントで持ちたくなったら、それは設計の誤り (1コンポーネント=1論理
  // リクエスト) を疑え。
  const activeRef = useRef<number | null>(null);
  const disposedRef = useRef(false);

  useEffect(() => {
    disposedRef.current = false;
    return () => {
      disposedRef.current = true;
    };
  }, []);

  return {
    /** 新リクエスト開始: 新 cid を採番し ref を上書きする */
    begin(): number {
      const cid = ++_cidSeq;
      // W-45: 旧 cid は即座に無効化される (超過リクエストの残留イベントは
      // 以降 accepts() で自動的に落ちる — 新リクエストが旧を supersede する
      // 意図的な設計)。
      activeRef.current = cid;
      return cid;
    },
    /** このリクエストが確定/失敗したら解除する (finally で必ず呼ぶこと) */
    end(cid: number) {
      if (activeRef.current === cid) activeRef.current = null;
    },
    /** このイベントを処理してよいか (disposed 後・cid 不一致は false) */
    accepts(payload: { cid?: number }): boolean {
      return (
        !disposedRef.current && payload.cid != null && payload.cid === activeRef.current
      );
    },
  };
}
