/** M3 Phase 3-B secure-vault UI: dumb view over the pure vault reducer. */

import {
  useEffect,
  useReducer,
  useRef,
  useState,
  type Dispatch,
  type ReactElement,
} from "react";

import {
  VaultIpcError,
  subscribeVaultEvents,
  vaultChatCreate,
  vaultChatsList,
  vaultLock,
  vaultMessageAppend,
  vaultMessagesList,
  vaultStatus,
  vaultUnlock,
  type VaultErrorCode,
  type VaultMessageAppendInput,
  type VaultMessageCursor,
  type VaultMessageRecord,
} from "../lib/vault";
import { cancelGeneration, generate } from "../lib/llm";
import { FOREGROUND_RESTORE_EVENT } from "../lib/foregroundRestore";
import {
  vaultErrorMessage,
  vaultStatusDescription,
  vaultStatusLabel,
} from "../lib/vaultErrorMessages";
import {
  shouldApplyVaultSnapshot,
  vaultPanelTone,
  vaultSystemErrorLine,
} from "../lib/vaultPanelView";
import {
  INITIAL_VAULT_STATE,
  vaultReducer,
  type VaultAction,
  type VaultState,
} from "../lib/vaultReducer";

const CHAT_LIST_LIMIT = 50;
const MESSAGE_PAGE_SIZE = 2;

// Fixed generation parameters for the vault chat assistant reply. Mirrors the
// PocketBrain defaults; tuning is a later concern.
const ASSISTANT_GENERATION = {
  n_ctx: 2048,
  max_tokens: 256,
  temp: 0.7,
  top_k: 40,
  top_p: 0.95,
  seed: 0,
} as const;

function errorCode(error: unknown): VaultErrorCode {
  return error instanceof VaultIpcError ? error.code : "unknown";
}

function nextCursorForPage(
  records: readonly VaultMessageRecord[],
): VaultMessageCursor | null {
  if (records.length < MESSAGE_PAGE_SIZE) {
    return null;
  }
  const last = records[records.length - 1];
  return last ? { timestamp: last.timestamp, id: last.id } : null;
}

function messageTime(timestamp: number): string {
  return new Date(timestamp).toLocaleString("ja-JP");
}

interface UnlockedVaultProps {
  readonly state: VaultState;
  readonly dispatch: Dispatch<VaultAction>;
}

function UnlockedVault(props: UnlockedVaultProps): ReactElement {
  const { state, dispatch } = props;
  const [title, setTitle] = useState("");
  const [selectedChatId, setSelectedChatId] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  // Streaming assistant reply lives ONLY in local state during generation; the
  // DB is never written per token (Phase 6-A Approach B).
  const [draft, setDraft] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [assistantFailed, setAssistantFailed] = useState(false);
  const pendingSave = useRef<VaultMessageAppendInput | null>(null);
  const mounted = useRef(true);

  // On unmount — which is how a fail-closed lock (`lockEngaged` → status ≠
  // "unlocked") tears down this view — force-stop any in-flight generation and
  // block the post-stream persist, so nothing is written to a locked vault.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      void cancelGeneration();
    };
  }, []);

  useEffect(() => {
    let active = true;
    dispatch({ type: "chatsLoadStarted" });
    void vaultChatsList({ limit: CHAT_LIST_LIMIT })
      .then((records) => {
        if (active) {
          dispatch({ type: "chatsLoaded", records });
        }
      })
      .catch((error: unknown) => {
        if (active) {
          dispatch({ type: "chatsLoadFailed", code: errorCode(error) });
        }
      });
    return () => {
      active = false;
    };
  }, [dispatch]);

  async function loadMessages(
    chatId: string,
    cursor: VaultMessageCursor | null,
  ): Promise<void> {
    dispatch({ type: "messagesLoadStarted", chatId, cursor });
    try {
      const records = await vaultMessagesList({
        chatId,
        cursor,
        limit: MESSAGE_PAGE_SIZE,
      });
      dispatch({
        type: "messagesLoaded",
        chatId,
        records,
        nextCursor: nextCursorForPage(records),
      });
    } catch (error: unknown) {
      dispatch({
        type: "messagesLoadFailed",
        chatId,
        code: errorCode(error),
      });
    }
  }

  async function onCreateChat(): Promise<void> {
    const nextTitle = title.trim();
    if (nextTitle.length === 0 || state.chats.phase === "loading") {
      return;
    }

    dispatch({ type: "chatsLoadStarted" });
    try {
      const created = await vaultChatCreate({
        id: crypto.randomUUID(),
        title: nextTitle,
        createdAt: Date.now(),
      });
      const records = await vaultChatsList({ limit: CHAT_LIST_LIMIT });
      dispatch({ type: "chatsLoaded", records });
      setTitle("");
      setSelectedChatId(created.id);
      await loadMessages(created.id, null);
    } catch (error: unknown) {
      dispatch({ type: "chatsLoadFailed", code: errorCode(error) });
    }
  }

  async function onSelectChat(chatId: string): Promise<void> {
    if (state.messages.phase === "loading") {
      return;
    }
    setSelectedChatId(chatId);
    await loadMessages(chatId, null);
  }

  // Single vault write (Approach B). Returns whether the append committed.
  async function persistMessage(
    input: VaultMessageAppendInput,
    retry: boolean,
  ): Promise<boolean> {
    if (retry) {
      dispatch({ type: "saveRetried" });
    } else {
      dispatch({ type: "savePending", messageId: input.id });
    }
    try {
      const record = await vaultMessageAppend(input);
      dispatch({ type: "saveSucceeded", messageId: input.id, record });
      pendingSave.current = null;
      return true;
    } catch (error: unknown) {
      dispatch({
        type: "saveFailed",
        messageId: input.id,
        code: errorCode(error),
      });
      return false;
    }
  }

  // Stream the assistant reply into local `draft` only, then persist the whole
  // completed text with ONE append — on normal finish and on cancel alike.
  async function streamAssistant(chatId: string, prompt: string): Promise<void> {
    setAssistantFailed(false);
    setDraft("");
    setStreaming(true);
    const chunks: string[] = [];
    try {
      await generate({ prompt, ...ASSISTANT_GENERATION }, (event) => {
        if (event.error !== null || event.done) {
          return;
        }
        chunks.push(event.text);
        setDraft((previous) => previous + event.text);
      });
    } catch {
      // Generation failed or was cancelled; persist whatever streamed so far.
    } finally {
      setStreaming(false);
      // The view may have been unmounted by a lock while streaming — never
      // write to a now-locked vault, and never persist orphaned plaintext.
      if (mounted.current) {
        const full = chunks.join("");
        if (full.length > 0) {
          const assistantInput: VaultMessageAppendInput = {
            id: crypto.randomUUID(),
            chatId,
            role: "assistant",
            content: full,
            timestamp: Date.now(),
          };
          pendingSave.current = assistantInput;
          const saved = await persistMessage(assistantInput, false);
          if (saved && mounted.current) {
            setDraft("");
            await loadMessages(chatId, null);
          }
        } else {
          setAssistantFailed(true);
          setDraft("");
        }
      }
    }
  }

  async function onSend(): Promise<void> {
    const content = message.trim();
    if (
      selectedChatId === null ||
      content.length === 0 ||
      streaming ||
      state.save.phase === "pending"
    ) {
      return;
    }
    setMessage("");
    const userInput: VaultMessageAppendInput = {
      id: crypto.randomUUID(),
      chatId: selectedChatId,
      role: "user",
      content,
      timestamp: Date.now(),
    };
    pendingSave.current = userInput;
    const saved = await persistMessage(userInput, false);
    if (saved && mounted.current) {
      await streamAssistant(selectedChatId, content);
    }
  }

  function onCancelStream(): void {
    void cancelGeneration();
  }

  async function onRetrySave(): Promise<void> {
    const input = pendingSave.current;
    if (
      state.save.phase !== "failed" ||
      input === null ||
      state.save.messageId !== input.id
    ) {
      return;
    }
    await persistMessage(input, true);
  }

  const messagesForSelection =
    selectedChatId !== null && state.messages.chatId === selectedChatId
      ? state.messages.items
      : [];

  return (
    <div className="vault-unlocked">
      <section className="vault-section" aria-labelledby="vault-chats-heading">
        <h3 id="vault-chats-heading">Chats</h3>
        <div className="vault-form-row">
          <label htmlFor="vault-chat-title">タイトル</label>
          <input
            id="vault-chat-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            maxLength={2048}
          />
          <button
            type="button"
            className="primary"
            onClick={() => void onCreateChat()}
            disabled={title.trim().length === 0 || state.chats.phase === "loading"}
          >
            作成
          </button>
        </div>

        {state.chats.error !== null && (
          <p className="error-text vault-error" role="alert">
            {vaultErrorMessage(state.chats.error)}
          </p>
        )}

        <ul className="vault-chat-list" aria-label="保管庫のチャット一覧">
          {state.chats.items.map((chat) => (
            <li key={chat.id}>
              <button
                type="button"
                className={selectedChatId === chat.id ? "active" : ""}
                aria-pressed={selectedChatId === chat.id}
                aria-label={`チャットを選択: ${chat.title}`}
                onClick={() => void onSelectChat(chat.id)}
                disabled={state.messages.phase === "loading" || streaming}
              >
                <span>{chat.title}</span>
                <time>{messageTime(chat.created_at)}</time>
              </button>
            </li>
          ))}
        </ul>
      </section>

      {selectedChatId !== null && (
        <section className="vault-section" aria-labelledby="vault-messages-heading">
          <h3 id="vault-messages-heading">Messages</h3>

          {state.messages.error !== null && (
            <p className="error-text vault-error" role="alert">
              {vaultErrorMessage(state.messages.error)}
            </p>
          )}

          <ol className="vault-message-list" aria-label="保管庫のメッセージ一覧">
            {messagesForSelection.map((record) => (
              <li key={record.id} data-message-id={record.id}>
                <header>
                  <span>{record.role}</span>
                  <time>{messageTime(record.timestamp)}</time>
                </header>
                <p>{record.content}</p>
              </li>
            ))}
          </ol>

          {streaming && (
            <div className="vault-draft" aria-live="polite" data-role="assistant-draft">
              <header>
                <span>assistant</span>
                <span>生成中…</span>
              </header>
              <p>{draft}</p>
            </div>
          )}

          {assistantFailed && (
            <p className="error-text vault-error" role="alert">
              応答を生成できませんでした。もう一度お試しください。
            </p>
          )}

          {state.messages.cursor !== null && (
            <button
              type="button"
              className="ghost"
              onClick={() => void loadMessages(selectedChatId, state.messages.cursor)}
              disabled={state.messages.phase === "loading" || streaming}
            >
              続きを読む
            </button>
          )}

          <div className="vault-message-compose">
            <label htmlFor="vault-message-input">メッセージ</label>
            <textarea
              id="vault-message-input"
              rows={3}
              value={message}
              onChange={(event) => setMessage(event.target.value)}
              maxLength={65_536}
              disabled={streaming}
            />
            <button
              type="button"
              className="primary"
              onClick={() => void onSend()}
              disabled={
                message.trim().length === 0 ||
                streaming ||
                state.save.phase === "pending"
              }
            >
              {state.save.phase === "pending" ? "保存中…" : "送信"}
            </button>
            {streaming && (
              <button type="button" className="ghost" onClick={onCancelStream}>
                停止
              </button>
            )}
          </div>

          {state.save.phase === "failed" && state.save.error !== null && (
            <div className="vault-retry" role="alert">
              <p className="error-text vault-error">
                {vaultErrorMessage(state.save.error)}
              </p>
              <button type="button" className="ghost" onClick={() => void onRetrySave()}>
                同じ識別子で再試行
              </button>
            </div>
          )}
        </section>
      )}
    </div>
  );
}

export interface VaultPanelProps {
  readonly variant?: "full" | "compact";
}

export function VaultPanel({
  variant = "full",
}: VaultPanelProps): ReactElement | null {
  const [state, dispatch] = useReducer(vaultReducer, INITIAL_VAULT_STATE);
  const [probe, setProbe] = useState<"pending" | "ready" | "absent">("pending");
  const [probeAttempt, setProbeAttempt] = useState(0);
  const lifecycleRevision = useRef(0);
  const statusSnapshotRequest = useRef(0);

  // Subscribe first so there is no status-probe → channel-registration gap.
  // The worker pushes its current status and also returns the same snapshot. If
  // event delivery wins, the revision guard prevents that snapshot from later
  // rewinding a newer lifecycle event.
  useEffect(() => {
    let active = true;
    const revisionAtSubscription = lifecycleRevision.current;
    void subscribeVaultEvents((event) => {
      if (!active) {
        return;
      }
      lifecycleRevision.current += 1;
      setProbe("ready");
      if (event.kind === "status") {
        dispatch({ type: "statusReceived", status: event.status });
      } else {
        dispatch({ type: "lockEngaged", code: event.code, status: event.status });
      }
    })
      .then((status) => {
        if (!active) {
          return;
        }
        setProbe("ready");
        if (
          shouldApplyVaultSnapshot(
            revisionAtSubscription,
            lifecycleRevision.current,
          )
        ) {
          dispatch({ type: "statusReceived", status });
        }
      })
      .catch(() => {
        if (active) {
          setProbe("absent");
        }
      });
    return () => {
      active = false;
    };
  }, [probeAttempt]);

  // Re-sync status when the app returns to the foreground. On iOS the native
  // lifecycle observer locks the vault on backgrounding; App's Phase 10 restore
  // coordinator probes first — this listens to that event (and still re-probes
  // on bare visibility as a belt-and-suspenders path for desktop VaultPanel).
  useEffect(() => {
    if (probe !== "ready") {
      return;
    }
    let active = true;
    function applyStatus(): void {
      const requestId = statusSnapshotRequest.current + 1;
      statusSnapshotRequest.current = requestId;
      const revisionAtRequest = lifecycleRevision.current;
      void vaultStatus()
        .then((status) => {
          if (
            active &&
            requestId === statusSnapshotRequest.current &&
            shouldApplyVaultSnapshot(
              revisionAtRequest,
              lifecycleRevision.current,
            )
          ) {
            dispatch({ type: "statusReceived", status });
          }
        })
        .catch(() => {
          // Keep the last known state on a failed re-probe (closed error policy).
        });
    }
    function onVisibility(): void {
      if (document.visibilityState !== "visible") {
        return;
      }
      applyStatus();
    }
    function onRestore(): void {
      applyStatus();
    }
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    return () => {
      active = false;
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    };
  }, [probe]);

  async function onUnlock(): Promise<void> {
    dispatch({ type: "unlockStarted" });
    try {
      const status = await vaultUnlock();
      dispatch({ type: "unlockSucceeded", status });
    } catch (error: unknown) {
      dispatch({ type: "unlockFailed", code: errorCode(error) });
    }
  }

  async function onLock(): Promise<void> {
    dispatch({ type: "lockStarted" });
    try {
      await vaultLock();
      dispatch({ type: "lockSucceeded" });
    } catch (error: unknown) {
      dispatch({ type: "lockFailed", code: errorCode(error) });
    }
  }

  const compact = variant === "compact";
  const headingId = compact ? "mobile-vault-heading" : "vault-heading";

  if (probe !== "ready" && !compact) {
    return null;
  }

  if (probe !== "ready") {
    const probing = probe === "pending";
    return (
      <section
        className="vault-panel vault-panel--compact term-panel"
        aria-labelledby={headingId}
        data-vault-tone={probing ? "active" : "error"}
      >
        <header className="vault-header">
          <div>
            <p className="term-header">Secure Vault // Bio-Gate</p>
            <h2 id={headingId}>保管庫ロック</h2>
          </div>
          <span className="vault-status" data-vault-status="unavailable">
            {probing ? "状態照会中" : "利用不可"}
          </span>
        </header>
        {probing ? (
          <p className="vault-status-description" role="status" aria-live="polite">
            生体認証ゲートを照会しています。
          </p>
        ) : (
          <>
            <p className="error-text vault-error" role="alert">
              {vaultSystemErrorLine("unavailable")}
            </p>
            <div className="vault-actions">
              <button
                type="button"
                className="ghost"
                onClick={() => {
                  setProbe("pending");
                  setProbeAttempt((attempt) => attempt + 1);
                }}
              >
                再走査
              </button>
            </div>
          </>
        )}
      </section>
    );
  }

  const showUnlock =
    state.status === "unprovisioned" ||
    state.status === "locked" ||
    state.status === "unlocking" ||
    state.status === "unavailable";
  const protectedStop =
    state.status === "recovery_required" ||
    state.status === "orphaned_key" ||
    state.status === "quarantined";
  const operationError = state.unlock.error ?? state.lock.error;
  const tone = vaultPanelTone(state.status, operationError);

  return (
    <section
      className={
        compact
          ? "vault-panel vault-panel--compact term-panel"
          : "vault-panel term-panel"
      }
      aria-labelledby={headingId}
      data-vault-tone={tone}
    >
      <header className="vault-header">
        <div>
          <p className="term-header">
            {compact ? "Secure Vault // Bio-Gate" : "Secure Vault"}
          </p>
          <h2 id={headingId}>{compact ? "保管庫ロック" : "暗号化保管庫"}</h2>
        </div>
        <span
          className="vault-status"
          data-vault-status={state.status}
          data-vault-tone={tone}
        >
          {vaultStatusLabel(state.status)}
        </span>
      </header>

      <p className="vault-status-description" role="status" aria-live="polite">
        {vaultStatusDescription(state.status)}
      </p>

      {!protectedStop && (
        <div className="vault-actions">
          {showUnlock && (
            <button
              type="button"
              className="primary vault-unlock-control"
              data-operation-state={state.unlock.phase}
              onClick={() => void onUnlock()}
              disabled={state.unlock.phase === "pending" || state.status === "unlocking"}
            >
              {state.status === "unlocking" ? "OS認証待ち…" : "ロック解除"}
            </button>
          )}
          {state.status === "unlocked" && (
            <button
              type="button"
              className="ghost"
              onClick={() => void onLock()}
              disabled={state.lock.phase === "pending"}
            >
              ロック
            </button>
          )}
        </div>
      )}

      {state.unlock.error !== null && (
        <p className="error-text vault-error" role="alert">
          {vaultSystemErrorLine(state.unlock.error)}
        </p>
      )}
      {state.lock.error !== null && (
        <p className="error-text vault-error" role="alert">
          {vaultSystemErrorLine(state.lock.error)}
        </p>
      )}

      {!compact && state.status === "unlocked" && (
        <UnlockedVault state={state} dispatch={dispatch} />
      )}
    </section>
  );
}
