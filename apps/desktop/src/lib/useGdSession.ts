//! Hook: GD Arena live session (Inner Coliseum). Builds the SPEAK-DSL system
//! prompt from GdSetupConfig, streams `generate()` (llm_generate — accepts a
//! raw FE-built prompt, so no Rust change is needed), and reparses the
//! accumulated buffer into per-speaker transcript bubbles on every token.
//! GD intentionally does NOT use the Coliseum interview's temp=0.1/seed-pinned
//! determinism (mentor_zpd.rs COLISEUM_GENERATION_TEMP): that collapses five
//! personas into one monotone voice. Persona separation comes from the
//! prompt (contrasting archetypes + few-shot), not temperature.

import { useCallback, useEffect, useRef, useReducer } from "react";

import { buildGdSystemPrompt, gdValidLetters, renderGdHistory } from "./gdPrompt";
import {
  gdArenaReducer,
  initialGdArenaState,
  type GdArenaState,
} from "./gdArenaReducer";
import type { GdSetupConfig } from "./gdSetupState";
import { parseGdStream, type GdTranscriptMessage, type GdTranscriptStage } from "./gdStreamParser";
import { cancelGeneration, generate } from "./llm";
import { createStreamTerminalGate } from "./streamTerminalGate";
import { uiErrorMessage } from "./uiErrorMessages";
import { useThrottledStream } from "./useThrottledStream";

const STREAM_TERMINAL_TIMEOUT_MS = 180_000;
const GD_STAGE: GdTranscriptStage = "DISCUSSION";
const GD_N_CTX = 2048;
const GD_MAX_TOKENS = 320;
const GD_TEMP = 0.45;
const GD_TOP_K = 40;
const GD_TOP_P = 0.9;
const GD_SEED = 14;

let nextTurn = 0;
function allocTurnId(prefix: string): string {
  nextTurn += 1;
  return `${prefix}-${nextTurn}`;
}

export interface UseGdSessionResult {
  state: GdArenaState;
  setInput: (value: string) => void;
  send: () => void;
}

export function useGdSession(config: GdSetupConfig): UseGdSessionResult {
  const [state, dispatch] = useReducer(gdArenaReducer, undefined, initialGdArenaState);
  const stateRef = useRef(state);
  stateRef.current = state;
  const configRef = useRef(config);
  configRef.current = config;
  const roundRef = useRef(0);
  const bufferRef = useRef("");
  const lettersRef = useRef<string[]>([]);

  const { push: pushChunk, drainAndStop, flushAndStop } = useThrottledStream(
    (piece) => {
      bufferRef.current += piece;
      const parsed = parseGdStream(bufferRef.current, {
        validLetters: lettersRef.current,
        stage: GD_STAGE,
        idPrefix: `r${roundRef.current}`,
      });
      dispatch({ type: "round_tokens", parsed });
    },
  );

  // Unmount cancels any in-flight generation (RagChatPanel W4 pattern).
  useEffect(() => {
    return () => {
      flushAndStop();
      if (stateRef.current.streaming) {
        void cancelGeneration().catch(() => {
          /* best-effort */
        });
      }
    };
  }, [flushAndStop]);

  const setInput = useCallback((value: string) => {
    dispatch({ type: "set_input", value });
  }, []);

  const send = useCallback(() => {
    const text = stateRef.current.input.trim();
    if (!text || stateRef.current.streaming) return;

    const cfg = configRef.current;
    lettersRef.current = gdValidLetters(cfg);
    const userMessage: GdTranscriptMessage = {
      turnId: allocTurnId("u"),
      role: "USER",
      stage: GD_STAGE,
      text,
    };
    dispatch({ type: "send_begin", userMessage });
    roundRef.current += 1;
    bufferRef.current = "";

    const history = [...stateRef.current.messages, userMessage];
    const prompt = [
      buildGdSystemPrompt(cfg),
      "",
      "# 直近の議論",
      renderGdHistory(history) || "（まだ発言なし）",
      "",
      "続けて、参加者として2〜4発言を @レター> 形式で出力せよ:",
    ].join("\n");

    const terminal = createStreamTerminalGate(STREAM_TERMINAL_TIMEOUT_MS);

    void (async () => {
      try {
        await generate(
          {
            prompt,
            n_ctx: GD_N_CTX,
            max_tokens: GD_MAX_TOKENS,
            temp: GD_TEMP,
            top_k: GD_TOP_K,
            top_p: GD_TOP_P,
            seed: GD_SEED,
          },
          (event) => {
            if (!terminal.isPending()) return;
            if (event.error) {
              flushAndStop();
              dispatch({ type: "send_failure", message: uiErrorMessage("GD_ARENA") });
              terminal.settle();
              return;
            }
            if (event.text) {
              pushChunk(event.text);
            }
            if (event.done) {
              drainAndStop();
              terminal.settle();
            }
          },
        );
        await terminal.promise;
      } catch {
        flushAndStop();
        dispatch({ type: "send_failure", message: uiErrorMessage("GD_ARENA") });
      } finally {
        terminal.abort();
        dispatch({ type: "send_end" });
      }
    })();
  }, [drainAndStop, flushAndStop, pushChunk]);

  return { state, setInput, send };
}
