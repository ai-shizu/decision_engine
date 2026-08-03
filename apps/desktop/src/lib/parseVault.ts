/**
 * Strict, dependency-free parsers for the M3 secure-vault IPC boundary.
 *
 * Every successful Tauri response enters the frontend as `unknown` and must
 * pass through this module before it reaches application state.
 */

type JsonObject = Record<string, unknown>;

export class VaultParseError extends Error {
  constructor(path: string, reason: string) {
    super(`${path}: ${reason}`);
    this.name = "VaultParseError";
  }
}

const VAULT_STATUSES = [
  "unprovisioned",
  "locked",
  "unlocking",
  "unlocked",
  "locking",
  "recovery_required",
  "orphaned_key",
  "quarantined",
  "unavailable",
] as const;

export type VaultStatus = (typeof VAULT_STATUSES)[number];

const KNOWN_VAULT_ERROR_CODES = [
  "locked",
  "busy",
  "timeout",
  "authentication_cancelled",
  "authentication_failed",
  "interaction_not_allowed",
  "keychain_unavailable",
  "corrupt_or_wrong_key",
  "unsupported_schema",
  "vault_quarantined",
  "unavailable",
  "invalid_input",
  "not_found",
  "conflict",
  "storage_failed",
  "os_lock_engaged",
] as const;

export type KnownVaultErrorCode = (typeof KNOWN_VAULT_ERROR_CODES)[number];
export type VaultErrorCode = KnownVaultErrorCode | "unknown";

export interface VaultChatRecord {
  readonly id: string;
  readonly title: string;
  readonly created_at: number;
}

export interface VaultMessageRecord {
  readonly id: string;
  readonly chat_id: string;
  readonly role: string;
  readonly content: string;
  readonly timestamp: number;
}

export interface VaultMessageCursor {
  readonly timestamp: number;
  readonly id: string;
}

export interface VaultChatCreateInput {
  readonly id: string;
  readonly title: string;
  readonly createdAt: number;
}

export interface VaultChatDeleteInput {
  readonly id: string;
}

export interface VaultChatsListInput {
  readonly limit?: number | null;
}

export interface VaultMessageAppendInput {
  readonly id: string;
  readonly chatId: string;
  readonly role: string;
  readonly content: string;
  readonly timestamp: number;
}

export interface VaultMessagesListInput {
  readonly chatId: string;
  readonly cursor?: VaultMessageCursor | null;
  readonly limit?: number | null;
}

export interface VaultChatCreateRequest {
  readonly id: string;
  readonly title: string;
  readonly created_at: number;
}

export interface VaultChatDeleteRequest {
  readonly id: string;
}

export interface VaultChatsListRequest {
  readonly limit: number | null;
}

export interface VaultMessageAppendRequest {
  readonly id: string;
  readonly chat_id: string;
  readonly role: string;
  readonly content: string;
  readonly timestamp: number;
}

export interface VaultMessagesListRequest {
  readonly chat_id: string;
  readonly cursor: VaultMessageCursor | null;
  readonly limit: number | null;
}

function fail(path: string, reason: string): never {
  throw new VaultParseError(path, reason);
}

function asObject(value: unknown, path: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(path, "expected object");
  }
  return value as JsonObject;
}

function requireFields(object: JsonObject, fields: readonly string[], path: string): void {
  for (const field of fields) {
    if (!Object.prototype.hasOwnProperty.call(object, field)) {
      fail(`${path}.${field}`, "missing field");
    }
  }
}

function asString(value: unknown, path: string): string {
  if (typeof value !== "string") {
    fail(path, "expected string");
  }
  return value;
}

function asSafeInteger(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    fail(path, "expected safe integer");
  }
  return value;
}

function asArray(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) {
    fail(path, "expected array");
  }
  return value;
}

export function parseVaultStatus(value: unknown): VaultStatus {
  if (
    typeof value !== "string" ||
    !VAULT_STATUSES.includes(value as VaultStatus)
  ) {
    fail("vault.status", "unexpected literal");
  }
  return value as VaultStatus;
}

/** Error paths never throw and never retain raw native error material. */
export function parseVaultErrorCode(value: unknown): VaultErrorCode {
  if (
    typeof value === "string" &&
    KNOWN_VAULT_ERROR_CODES.includes(value as KnownVaultErrorCode)
  ) {
    return value as KnownVaultErrorCode;
  }
  return "unknown";
}

export function parseVaultChatRecord(value: unknown): VaultChatRecord {
  const object = asObject(value, "vault.chat");
  requireFields(object, ["id", "title", "created_at"], "vault.chat");
  return {
    id: asString(object.id, "vault.chat.id"),
    title: asString(object.title, "vault.chat.title"),
    created_at: asSafeInteger(object.created_at, "vault.chat.created_at"),
  };
}

export function parseVaultChatRecords(value: unknown): VaultChatRecord[] {
  return asArray(value, "vault.chats").map((record, index) =>
    parseVaultChatRecordAt(record, `vault.chats[${index}]`),
  );
}

function parseVaultChatRecordAt(value: unknown, path: string): VaultChatRecord {
  const object = asObject(value, path);
  requireFields(object, ["id", "title", "created_at"], path);
  return {
    id: asString(object.id, `${path}.id`),
    title: asString(object.title, `${path}.title`),
    created_at: asSafeInteger(object.created_at, `${path}.created_at`),
  };
}

export function parseVaultMessageRecord(value: unknown): VaultMessageRecord {
  return parseVaultMessageRecordAt(value, "vault.message");
}

function parseVaultMessageRecordAt(value: unknown, path: string): VaultMessageRecord {
  const object = asObject(value, path);
  requireFields(object, ["id", "chat_id", "role", "content", "timestamp"], path);
  return {
    id: asString(object.id, `${path}.id`),
    chat_id: asString(object.chat_id, `${path}.chat_id`),
    role: asString(object.role, `${path}.role`),
    content: asString(object.content, `${path}.content`),
    timestamp: asSafeInteger(object.timestamp, `${path}.timestamp`),
  };
}

export function parseVaultMessageRecords(value: unknown): VaultMessageRecord[] {
  return asArray(value, "vault.messages").map((record, index) =>
    parseVaultMessageRecordAt(record, `vault.messages[${index}]`),
  );
}

export function parseVaultUnit(value: unknown): void {
  if (value !== null) {
    fail("vault.unit", "expected null");
  }
}

/** Push event from the worker's lifecycle sink (mirrors the Rust enum). */
export type VaultLifecycleEvent =
  | { readonly kind: "status"; readonly status: VaultStatus }
  | { readonly kind: "error"; readonly code: VaultErrorCode; readonly status: VaultStatus };

/**
 * Strictly parse a lifecycle event. Malformed/unknown shapes throw
 * `VaultParseError` (the caller ignores them); a status field must be one of the
 * closed states, while an error code is normalized (never throws for the code).
 */
export function parseVaultLifecycleEvent(value: unknown): VaultLifecycleEvent {
  const object = asObject(value, "vault.event");
  requireFields(object, ["kind"], "vault.event");
  const kind = asString(object.kind, "vault.event.kind");
  if (kind === "status") {
    requireFields(object, ["status"], "vault.event");
    return { kind: "status", status: parseVaultStatus(object.status) };
  }
  if (kind === "error") {
    requireFields(object, ["code", "status"], "vault.event");
    return {
      kind: "error",
      code: parseVaultErrorCode(object.code),
      status: parseVaultStatus(object.status),
    };
  }
  return fail("vault.event.kind", "unexpected kind");
}

export function buildChatCreateRequest(
  input: VaultChatCreateInput,
): VaultChatCreateRequest {
  return {
    id: input.id,
    title: input.title,
    created_at: input.createdAt,
  };
}

export function buildChatDeleteRequest(
  input: VaultChatDeleteInput,
): VaultChatDeleteRequest {
  return { id: input.id };
}

export function buildChatsListRequest(
  input: VaultChatsListInput = {},
): VaultChatsListRequest {
  return { limit: input.limit ?? null };
}

export function buildMessageAppendRequest(
  input: VaultMessageAppendInput,
): VaultMessageAppendRequest {
  return {
    id: input.id,
    chat_id: input.chatId,
    role: input.role,
    content: input.content,
    timestamp: input.timestamp,
  };
}

export function buildMessagesListRequest(
  input: VaultMessagesListInput,
): VaultMessagesListRequest {
  return {
    chat_id: input.chatId,
    cursor: input.cursor ?? null,
    limit: input.limit ?? null,
  };
}
