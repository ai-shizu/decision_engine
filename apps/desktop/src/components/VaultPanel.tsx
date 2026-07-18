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
import {
  vaultErrorMessage,
  vaultStatusDescription,
  vaultStatusLabel,
} from "../lib/vaultErrorMessages";
import {
  INITIAL_VAULT_STATE,
  vaultReducer,
  type VaultAction,
  type VaultState,
} from "../lib/vaultReducer";

const CHAT_LIST_LIMIT = 50;
const MESSAGE_PAGE_SIZE = 2;

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
  const pendingSave = useRef<VaultMessageAppendInput | null>(null);

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

  async function persistMessage(
    input: VaultMessageAppendInput,
    retry: boolean,
  ): Promise<void> {
    if (retry) {
      dispatch({ type: "saveRetried" });
    } else {
      dispatch({ type: "savePending", messageId: input.id });
    }
    try {
      const record = await vaultMessageAppend(input);
      dispatch({ type: "saveSucceeded", messageId: input.id, record });
      pendingSave.current = null;
      setMessage("");
    } catch (error: unknown) {
      dispatch({
        type: "saveFailed",
        messageId: input.id,
        code: errorCode(error),
      });
    }
  }

  async function onSaveMessage(): Promise<void> {
    const content = message.trim();
    if (
      selectedChatId === null ||
      content.length === 0 ||
      state.save.phase === "pending"
    ) {
      return;
    }
    const input: VaultMessageAppendInput = {
      id: crypto.randomUUID(),
      chatId: selectedChatId,
      role: "user",
      content,
      timestamp: Date.now(),
    };
    pendingSave.current = input;
    await persistMessage(input, false);
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
                disabled={state.messages.phase === "loading"}
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

          {state.messages.cursor !== null && (
            <button
              type="button"
              className="ghost"
              onClick={() => void loadMessages(selectedChatId, state.messages.cursor)}
              disabled={state.messages.phase === "loading"}
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
            />
            <button
              type="button"
              className="primary"
              onClick={() => void onSaveMessage()}
              disabled={message.trim().length === 0 || state.save.phase === "pending"}
            >
              {state.save.phase === "pending" ? "保存中…" : "保存"}
            </button>
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

export function VaultPanel(): ReactElement | null {
  const [state, dispatch] = useReducer(vaultReducer, INITIAL_VAULT_STATE);
  const [probe, setProbe] = useState<"pending" | "ready" | "absent">("pending");

  useEffect(() => {
    let active = true;
    void vaultStatus()
      .then((status) => {
        if (active) {
          dispatch({ type: "statusReceived", status });
          setProbe("ready");
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
  }, []);

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

  if (probe !== "ready") {
    return null;
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

  return (
    <section className="vault-panel term-panel" aria-labelledby="vault-heading">
      <header className="vault-header">
        <div>
          <p className="term-header">Secure Vault</p>
          <h2 id="vault-heading">暗号化保管庫</h2>
        </div>
        <span className="vault-status" data-vault-status={state.status}>
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
              className="primary"
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
          {vaultErrorMessage(state.unlock.error)}
        </p>
      )}
      {state.lock.error !== null && (
        <p className="error-text vault-error" role="alert">
          {vaultErrorMessage(state.lock.error)}
        </p>
      )}

      {state.status === "unlocked" && (
        <UnlockedVault state={state} dispatch={dispatch} />
      )}
    </section>
  );
}
