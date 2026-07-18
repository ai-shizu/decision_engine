/** Pure state machine for the M3 vault UI boundary. */

import type {
  VaultChatRecord,
  VaultErrorCode,
  VaultMessageCursor,
  VaultMessageRecord,
  VaultStatus,
} from "./parseVault";

const MAX_REDUCER_RECORDS = 200;

type OperationPhase = "idle" | "pending" | "failed";
type LoadPhase = "idle" | "loading" | "loaded" | "failed";
type SavePhase = "idle" | "pending" | "saved" | "failed";

export interface VaultOperationState {
  readonly phase: OperationPhase;
  readonly error: VaultErrorCode | null;
}

export interface VaultChatsState {
  readonly phase: LoadPhase;
  readonly items: readonly VaultChatRecord[];
  readonly error: VaultErrorCode | null;
}

export interface VaultMessagesState {
  readonly phase: LoadPhase;
  readonly chatId: string | null;
  readonly items: readonly VaultMessageRecord[];
  readonly cursor: VaultMessageCursor | null;
  readonly error: VaultErrorCode | null;
}

export interface VaultSaveState {
  readonly phase: SavePhase;
  readonly messageId: string | null;
  readonly error: VaultErrorCode | null;
}

export interface VaultState {
  readonly status: VaultStatus;
  readonly unlock: VaultOperationState;
  readonly lock: VaultOperationState;
  readonly chats: VaultChatsState;
  readonly messages: VaultMessagesState;
  readonly save: VaultSaveState;
}

export type VaultAction =
  | { readonly type: "statusReceived"; readonly status: VaultStatus }
  | { readonly type: "unlockStarted" }
  | { readonly type: "unlockSucceeded"; readonly status: VaultStatus }
  | { readonly type: "unlockFailed"; readonly code: VaultErrorCode }
  | { readonly type: "lockStarted" }
  | { readonly type: "lockSucceeded" }
  | { readonly type: "lockFailed"; readonly code: VaultErrorCode }
  | { readonly type: "chatsLoadStarted" }
  | { readonly type: "chatsLoaded"; readonly records: readonly VaultChatRecord[] }
  | { readonly type: "chatsLoadFailed"; readonly code: VaultErrorCode }
  | {
      readonly type: "messagesLoadStarted";
      readonly chatId: string;
      readonly cursor: VaultMessageCursor | null;
    }
  | {
      readonly type: "messagesLoaded";
      readonly chatId: string;
      readonly records: readonly VaultMessageRecord[];
      readonly nextCursor: VaultMessageCursor | null;
    }
  | {
      readonly type: "messagesLoadFailed";
      readonly chatId: string;
      readonly code: VaultErrorCode;
    }
  | { readonly type: "savePending"; readonly messageId: string }
  | {
      readonly type: "saveSucceeded";
      readonly messageId: string;
      readonly record: VaultMessageRecord;
    }
  | {
      readonly type: "saveFailed";
      readonly messageId: string;
      readonly code: VaultErrorCode;
    }
  | { readonly type: "saveRetried" };

function idleOperation(): VaultOperationState {
  return { phase: "idle", error: null };
}

function emptyChats(): VaultChatsState {
  return { phase: "idle", items: [], error: null };
}

function emptyMessages(): VaultMessagesState {
  return {
    phase: "idle",
    chatId: null,
    items: [],
    cursor: null,
    error: null,
  };
}

function idleSave(): VaultSaveState {
  return { phase: "idle", messageId: null, error: null };
}

export const INITIAL_VAULT_STATE: VaultState = {
  status: "unprovisioned",
  unlock: idleOperation(),
  lock: idleOperation(),
  chats: emptyChats(),
  messages: emptyMessages(),
  save: idleSave(),
};

function withStatus(state: VaultState, status: VaultStatus): VaultState {
  if (status === "unlocked") {
    return { ...state, status };
  }
  return {
    ...state,
    status,
    chats: emptyChats(),
    messages: emptyMessages(),
    save: idleSave(),
  };
}

function statusAfterUnlockFailure(code: VaultErrorCode): VaultStatus {
  if (
    code === "vault_quarantined" ||
    code === "corrupt_or_wrong_key" ||
    code === "unsupported_schema"
  ) {
    return "quarantined";
  }
  if (code === "keychain_unavailable" || code === "unavailable") {
    return "unavailable";
  }
  return "locked";
}

function boundedHead<T>(items: readonly T[]): readonly T[] {
  return items.slice(0, MAX_REDUCER_RECORDS);
}

function compareMessages(left: VaultMessageRecord, right: VaultMessageRecord): number {
  if (left.timestamp !== right.timestamp) {
    return left.timestamp < right.timestamp ? -1 : 1;
  }
  if (left.id === right.id) {
    return 0;
  }
  return left.id < right.id ? -1 : 1;
}

function mergeMessages(
  existing: readonly VaultMessageRecord[],
  incoming: readonly VaultMessageRecord[],
): readonly VaultMessageRecord[] {
  const byId = new Map<string, VaultMessageRecord>();
  for (const record of existing) {
    byId.set(record.id, record);
  }
  for (const record of incoming) {
    byId.set(record.id, record);
  }
  return [...byId.values()]
    .sort(compareMessages)
    .slice(-MAX_REDUCER_RECORDS);
}

export function vaultReducer(state: VaultState, action: VaultAction): VaultState {
  switch (action.type) {
    case "statusReceived":
      return withStatus(state, action.status);

    case "unlockStarted": {
      if (
        (state.status !== "locked" &&
          state.status !== "unprovisioned" &&
          state.status !== "unavailable") ||
        state.unlock.phase === "pending"
      ) {
        return state;
      }
      const next = withStatus(state, "unlocking");
      return {
        ...next,
        unlock: { phase: "pending", error: null },
        lock: idleOperation(),
      };
    }

    case "unlockSucceeded": {
      if (state.unlock.phase !== "pending") {
        return state;
      }
      const next = withStatus(state, action.status);
      return { ...next, unlock: idleOperation() };
    }

    case "unlockFailed": {
      if (state.unlock.phase !== "pending") {
        return state;
      }
      const next = withStatus(state, statusAfterUnlockFailure(action.code));
      return {
        ...next,
        unlock: { phase: "failed", error: action.code },
      };
    }

    case "lockStarted": {
      if (state.status !== "unlocked" || state.lock.phase === "pending") {
        return state;
      }
      const next = withStatus(state, "locking");
      return {
        ...next,
        unlock: idleOperation(),
        lock: { phase: "pending", error: null },
      };
    }

    case "lockSucceeded": {
      if (state.lock.phase !== "pending") {
        return state;
      }
      const next = withStatus(state, "locked");
      return { ...next, lock: idleOperation() };
    }

    case "lockFailed": {
      if (state.lock.phase !== "pending") {
        return state;
      }
      const next = withStatus(state, "unavailable");
      return {
        ...next,
        lock: { phase: "failed", error: action.code },
      };
    }

    case "chatsLoadStarted":
      if (state.status !== "unlocked" || state.chats.phase === "loading") {
        return state;
      }
      return {
        ...state,
        chats: { ...state.chats, phase: "loading", error: null },
      };

    case "chatsLoaded":
      if (state.status !== "unlocked" || state.chats.phase !== "loading") {
        return state;
      }
      return {
        ...state,
        chats: { phase: "loaded", items: boundedHead(action.records), error: null },
      };

    case "chatsLoadFailed":
      if (state.status !== "unlocked" || state.chats.phase !== "loading") {
        return state;
      }
      return {
        ...state,
        chats: { ...state.chats, phase: "failed", error: action.code },
      };

    case "messagesLoadStarted":
      if (state.status !== "unlocked" || state.messages.phase === "loading") {
        return state;
      }
      return {
        ...state,
        messages: {
          phase: "loading",
          chatId: action.chatId,
          items: state.messages.chatId === action.chatId ? state.messages.items : [],
          cursor: action.cursor,
          error: null,
        },
      };

    case "messagesLoaded":
      if (
        state.status !== "unlocked" ||
        state.messages.phase !== "loading" ||
        state.messages.chatId !== action.chatId
      ) {
        return state;
      }
      return {
        ...state,
        messages: {
          phase: "loaded",
          chatId: action.chatId,
          items:
            state.messages.cursor === null
              ? mergeMessages([], action.records)
              : mergeMessages(state.messages.items, action.records),
          cursor: action.nextCursor,
          error: null,
        },
      };

    case "messagesLoadFailed":
      if (
        state.status !== "unlocked" ||
        state.messages.phase !== "loading" ||
        state.messages.chatId !== action.chatId
      ) {
        return state;
      }
      return {
        ...state,
        messages: {
          ...state.messages,
          phase: "failed",
          error: action.code,
        },
      };

    case "savePending":
      if (state.status !== "unlocked" || state.save.phase === "pending") {
        return state;
      }
      return {
        ...state,
        save: { phase: "pending", messageId: action.messageId, error: null },
      };

    case "saveSucceeded": {
      if (
        state.status !== "unlocked" ||
        state.save.phase !== "pending" ||
        state.save.messageId !== action.messageId
      ) {
        return state;
      }
      const sameChat = state.messages.chatId === action.record.chat_id;
      const withoutReplay = sameChat
        ? state.messages.items.filter((record) => record.id !== action.record.id)
        : state.messages.items;
      return {
        ...state,
        messages: sameChat
          ? {
              ...state.messages,
              items: mergeMessages(withoutReplay, [action.record]),
            }
          : state.messages,
        save: { phase: "saved", messageId: action.messageId, error: null },
      };
    }

    case "saveFailed":
      if (
        state.status !== "unlocked" ||
        state.save.phase !== "pending" ||
        state.save.messageId !== action.messageId
      ) {
        return state;
      }
      return {
        ...state,
        save: {
          phase: "failed",
          messageId: action.messageId,
          error: action.code,
        },
      };

    case "saveRetried":
      if (
        state.status !== "unlocked" ||
        state.save.phase !== "failed" ||
        state.save.messageId === null
      ) {
        return state;
      }
      return {
        ...state,
        save: {
          phase: "pending",
          messageId: state.save.messageId,
          error: null,
        },
      };

    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
}
