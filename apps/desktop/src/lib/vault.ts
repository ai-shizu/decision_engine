/** The sole frontend owner of M3 secure-vault Tauri IPC calls. */

import { invoke } from "@tauri-apps/api/core";

import {
  buildChatCreateRequest,
  buildChatDeleteRequest,
  buildChatsListRequest,
  buildMessageAppendRequest,
  buildMessagesListRequest,
  parseVaultChatRecord,
  parseVaultChatRecords,
  parseVaultErrorCode,
  parseVaultMessageRecord,
  parseVaultMessageRecords,
  parseVaultStatus,
  parseVaultUnit,
  type VaultChatCreateInput,
  type VaultChatDeleteInput,
  type VaultChatRecord,
  type VaultChatsListInput,
  type VaultErrorCode,
  type VaultMessageAppendInput,
  type VaultMessageRecord,
  type VaultMessagesListInput,
  type VaultStatus,
} from "./parseVault";

export type {
  VaultChatCreateInput,
  VaultChatDeleteInput,
  VaultChatRecord,
  VaultChatsListInput,
  VaultErrorCode,
  VaultMessageAppendInput,
  VaultMessageCursor,
  VaultMessageRecord,
  VaultMessagesListInput,
  VaultStatus,
} from "./parseVault";

type VaultIpcCommand =
  | "vault_status"
  | "vault_unlock"
  | "vault_lock"
  | "check_db_health"
  | "vault_chat_create"
  | "vault_chat_delete"
  | "vault_chats_list"
  | "vault_message_append"
  | "vault_messages_list";

type VaultParser<T> = (value: unknown) => T;

export class VaultIpcError extends Error {
  readonly code: VaultErrorCode;

  constructor(code: VaultErrorCode) {
    super("Vault request failed");
    this.name = "VaultIpcError";
    this.code = code;
  }
}

async function invokeVault<T>(
  command: VaultIpcCommand,
  parser: VaultParser<T>,
  request?: object,
): Promise<T> {
  let raw: unknown;
  try {
    raw = request === undefined
      ? await invoke<unknown>(command)
      : await invoke<unknown>(command, { request });
  } catch (error: unknown) {
    throw new VaultIpcError(parseVaultErrorCode(error));
  }
  return parser(raw);
}

export function vaultStatus(): Promise<VaultStatus> {
  return invokeVault("vault_status", parseVaultStatus);
}

export function vaultUnlock(): Promise<VaultStatus> {
  return invokeVault("vault_unlock", parseVaultStatus);
}

export function vaultLock(): Promise<VaultStatus> {
  return invokeVault("vault_lock", parseVaultStatus);
}

export function checkDbHealth(): Promise<VaultStatus> {
  return invokeVault("check_db_health", parseVaultStatus);
}

export function vaultChatCreate(
  input: VaultChatCreateInput,
): Promise<VaultChatRecord> {
  return invokeVault(
    "vault_chat_create",
    parseVaultChatRecord,
    buildChatCreateRequest(input),
  );
}

export function vaultChatDelete(input: VaultChatDeleteInput): Promise<void> {
  return invokeVault(
    "vault_chat_delete",
    parseVaultUnit,
    buildChatDeleteRequest(input),
  );
}

export function vaultChatsList(
  input: VaultChatsListInput = {},
): Promise<VaultChatRecord[]> {
  return invokeVault(
    "vault_chats_list",
    parseVaultChatRecords,
    buildChatsListRequest(input),
  );
}

export function vaultMessageAppend(
  input: VaultMessageAppendInput,
): Promise<VaultMessageRecord> {
  return invokeVault(
    "vault_message_append",
    parseVaultMessageRecord,
    buildMessageAppendRequest(input),
  );
}

export function vaultMessagesList(
  input: VaultMessagesListInput,
): Promise<VaultMessageRecord[]> {
  return invokeVault(
    "vault_messages_list",
    parseVaultMessageRecords,
    buildMessagesListRequest(input),
  );
}
