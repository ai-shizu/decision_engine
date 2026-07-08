import { useCallback, useEffect, useRef } from "react";

/**
 * F3.5 (SPEC_FOXTROT_UI.md §7 裁定1): chunk ストリームを一定速で放出する
 * 共有フック。CONSULT と INTERVIEW で共用する (keyUtils と同じ「1関数を
 * 全員が通る」規律)。
 *
 * アーキテクチャ: 文字キュー (ref) + 単一 interval タイマー
 * (コンポーネントインスタンスごとに1個。chunk毎の setTimeout 乱立は
 * W-36 違反)。
 *
 * F-14 (ランダムジッタ禁止): 「人間らしさ」のための揺らぎは偽の
 * ランダム性であり違法。放出レートは定数のみ — UI から変更させない。
 */
const THROTTLE_TICK_MS = 30;
const THROTTLE_CHARS_PER_TICK = 2;

export function useThrottledStream(onEmit: (chunk: string) => void) {
  const queueRef = useRef("");
  const timerRef = useRef<number | null>(null);
  // 最新の onEmit を常に反映する (呼び出し側で useCallback を強制しない)。
  const onEmitRef = useRef(onEmit);
  onEmitRef.current = onEmit;

  const stopTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const ensureTimer = useCallback(() => {
    if (timerRef.current !== null) return; // W-36: interval は1個のみ
    timerRef.current = window.setInterval(() => {
      if (!queueRef.current) {
        stopTimer();
        return;
      }
      const piece = queueRef.current.slice(0, THROTTLE_CHARS_PER_TICK);
      queueRef.current = queueRef.current.slice(piece.length);
      onEmitRef.current(piece);
    }, THROTTLE_TICK_MS);
  }, [stopTimer]);

  /** chunk 受信のたびに呼ぶ。文字をキューへ積み、タイマーが未起動なら起動する。 */
  const push = useCallback(
    (text: string) => {
      queueRef.current += text;
      ensureTimer();
    },
    [ensureTimer],
  );

  /**
   * W-35: 確定置換の直前に必ず呼ぶこと。キュー破棄 → (呼び出し側の)
   * 置換 setState、の原子的な順序を保証する。破棄せずに置換すると、
   * 置換後のメッセージへ古いキューの残りが追記され続ける。
   */
  const flushAndStop = useCallback(() => {
    queueRef.current = "";
    stopTimer();
  }, [stopTimer]);

  // W-36: unmount 時にタイマーを確実に破棄する (StrictMode 二重実行対策込み)。
  useEffect(() => stopTimer, [stopTimer]);

  return { push, flushAndStop };
}
