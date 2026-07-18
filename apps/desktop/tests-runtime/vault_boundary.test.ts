import {
  VaultParseError,
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
  type VaultErrorCode,
  type VaultStatus,
} from "../src/lib/parseVault";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function assertEq<T>(actual: T, expected: T, message: string): void {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

function assertDeepEq(actual: unknown, expected: unknown, message: string): void {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function assertParseError(fn: () => unknown, message: string): void {
  try {
    fn();
  } catch (error: unknown) {
    assertOk(error instanceof VaultParseError, `${message}: wrong error type`);
    return;
  }
  throw new Error(`${message}: expected parse error`);
}

const STATUSES: readonly VaultStatus[] = [
  "unprovisioned",
  "locked",
  "unlocking",
  "unlocked",
  "locking",
  "recovery_required",
  "orphaned_key",
  "quarantined",
  "unavailable",
];

const ERROR_CODES: readonly Exclude<VaultErrorCode, "unknown">[] = [
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
];

const CHAT_ID = "00000000-0000-4000-8000-000000000001";
const MESSAGE_ID = "10000000-0000-4000-8000-000000000001";

test("V-B01 accepts all nine closed vault statuses", () => {
  for (const status of STATUSES) {
    assertEq(parseVaultStatus(status), status, status);
  }
});

test("V-B02 rejects unknown and non-string vault statuses", () => {
  assertParseError(() => parseVaultStatus("future_state"), "unknown status");
  assertParseError(() => parseVaultStatus(null), "null status");
});

test("V-B03 preserves all fifteen known error codes", () => {
  for (const code of ERROR_CODES) {
    assertEq(parseVaultErrorCode(code), code, code);
  }
});

test("V-B04 normalizes unknown error material without throwing", () => {
  assertEq(parseVaultErrorCode("native secret"), "unknown", "unknown string");
  assertEq(parseVaultErrorCode(new Error("native secret")), "unknown", "Error object");
  assertEq(parseVaultErrorCode({ code: "locked" }), "unknown", "object");
  assertEq(parseVaultErrorCode(null), "unknown", "null");
});

test("V-B05 parses chat records and ignores extra response fields", () => {
  const parsed = parseVaultChatRecord({
    id: CHAT_ID,
    title: "chat",
    created_at: 1,
    ignored: "server extension",
  });
  assertDeepEq(parsed, { id: CHAT_ID, title: "chat", created_at: 1 }, "chat");
  assertDeepEq(parseVaultChatRecords([parsed]), [parsed], "chat list");
});

test("V-B06 rejects missing and mistyped chat fields", () => {
  assertParseError(
    () => parseVaultChatRecord({ id: CHAT_ID, title: "chat" }),
    "missing created_at",
  );
  assertParseError(
    () => parseVaultChatRecord({ id: CHAT_ID, title: "chat", created_at: "1" }),
    "mistyped created_at",
  );
});

test("V-B07 parses message records and ignores extra response fields", () => {
  const parsed = parseVaultMessageRecord({
    id: MESSAGE_ID,
    chat_id: CHAT_ID,
    role: "user",
    content: "hello",
    timestamp: 2,
    ignored: true,
  });
  assertDeepEq(
    parsed,
    {
      id: MESSAGE_ID,
      chat_id: CHAT_ID,
      role: "user",
      content: "hello",
      timestamp: 2,
    },
    "message",
  );
  assertDeepEq(parseVaultMessageRecords([parsed]), [parsed], "message list");
});

test("V-B08 rejects missing and mistyped message fields", () => {
  assertParseError(
    () =>
      parseVaultMessageRecord({
        id: MESSAGE_ID,
        chat_id: CHAT_ID,
        role: "user",
        timestamp: 2,
      }),
    "missing content",
  );
  assertParseError(
    () =>
      parseVaultMessageRecord({
        id: MESSAGE_ID,
        chat_id: CHAT_ID,
        role: "user",
        content: "hello",
        timestamp: 2.5,
      }),
    "non-integer timestamp",
  );
});

test("V-B09 request builders emit exact snake_case wire keys", () => {
  assertDeepEq(
    buildChatCreateRequest({ id: CHAT_ID, title: "chat", createdAt: 1 }),
    { id: CHAT_ID, title: "chat", created_at: 1 },
    "chat create",
  );
  assertDeepEq(buildChatDeleteRequest({ id: CHAT_ID }), { id: CHAT_ID }, "chat delete");
  assertDeepEq(buildChatsListRequest({ limit: 25 }), { limit: 25 }, "chats list");
  assertDeepEq(
    buildMessageAppendRequest({
      id: MESSAGE_ID,
      chatId: CHAT_ID,
      role: "assistant",
      content: "reply",
      timestamp: 2,
    }),
    {
      id: MESSAGE_ID,
      chat_id: CHAT_ID,
      role: "assistant",
      content: "reply",
      timestamp: 2,
    },
    "message append",
  );
  assertDeepEq(
    buildMessagesListRequest({
      chatId: CHAT_ID,
      cursor: { timestamp: 2, id: MESSAGE_ID },
      limit: 50,
    }),
    {
      chat_id: CHAT_ID,
      cursor: { timestamp: 2, id: MESSAGE_ID },
      limit: 50,
    },
    "messages list",
  );
});

test("V-B10 optional request fields become explicit nulls", () => {
  assertDeepEq(buildChatsListRequest(), { limit: null }, "default list limit");
  assertDeepEq(
    buildMessagesListRequest({ chatId: CHAT_ID }),
    { chat_id: CHAT_ID, cursor: null, limit: null },
    "default messages list",
  );
});

test("V-B11 unit responses require Rust unit's null encoding", () => {
  parseVaultUnit(null);
  assertParseError(() => parseVaultUnit(undefined), "undefined unit");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log(`PASS ${name}`);
  } catch (error: unknown) {
    failed += 1;
    console.error(`FAIL ${name}`, error);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
