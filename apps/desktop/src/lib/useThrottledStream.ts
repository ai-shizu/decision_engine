import { useCallback, useEffect, useRef } from "react";

/**
 * F3.5 (SPEC_FOXTROT_UI.md §7 裁定1): chunk ストリームを一定速で放出する
 * 共有フック。CONSULT と INTERVIEW で共用する (keyUtils と同じ「1関数を
 * 全員が通る」規律)。
 *
 * アーキテクチャ: chunk 配列キュー (ref) + 単一 timeout。受信済みの全文を
 * 2文字ずつ slice する旧方式は、各consumerの全文parse/renderをO(n²)化して
 * いた。現在は1フレーム相当の時間に届いたchunkを1回でjoin/emitする。
 *
 * F-14 (ランダムジッタ禁止): 「人間らしさ」のための揺らぎは偽の
 * ランダム性であり違法。放出レートは定数のみ — UI から変更させない。
 */
const STREAM_BATCH_MS = 50;

export function useThrottledStream(onEmit: (chunk: string) => void) {
  const queueRef = useRef<string[]>([]);
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

  const emitQueued = useCallback(() => {
    if (queueRef.current.length === 0) return;
    const batch = queueRef.current.join("");
    queueRef.current = [];
    if (batch) {
      onEmitRef.current(batch);
    }
  }, []);

  const ensureTimer = useCallback(() => {
    if (timerRef.current !== null) return; // W-36: timer は1個のみ
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      emitQueued();
    }, STREAM_BATCH_MS);
  }, [emitQueued]);

  /** chunk受信のたびに呼ぶ。同一batch窓のchunkは1回のReact更新へ畳み込む。 */
  const push = useCallback(
    (text: string) => {
      if (!text) return;
      queueRef.current.push(text);
      ensureTimer();
    },
    [ensureTimer],
  );

  /**
   * Normal terminal path: synchronously emit the queued tail before stopping.
   * Channel delivery can outpace the 30 ms display cadence, so `done` commonly
   * arrives while characters remain queued. Dropping that tail truncates an
   * otherwise successful model response.
   */
  const drainAndStop = useCallback(() => {
    stopTimer();
    emitQueued();
  }, [emitQueued, stopTimer]);

  /**
   * W-35: 確定置換の直前に必ず呼ぶこと。キュー破棄 → (呼び出し側の)
   * 置換 setState、の原子的な順序を保証する。破棄せずに置換すると、
   * 置換後のメッセージへ古いキューの残りが追記され続ける。
   */
  const flushAndStop = useCallback(() => {
    queueRef.current = [];
    stopTimer();
  }, [stopTimer]);

  // W-36: unmount 時にタイマーを確実に破棄する (StrictMode 二重実行対策込み)。
  useEffect(() => stopTimer, [stopTimer]);

  return { push, drainAndStop, flushAndStop };
}
