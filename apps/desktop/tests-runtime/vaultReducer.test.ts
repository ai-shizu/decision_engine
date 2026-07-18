import {
  INITIAL_VAULT_STATE,
  vaultReducer,
  type VaultState,
} from "../src/lib/vaultReducer";
import type {
  VaultChatRecord,
  VaultMessageRecord,
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

const CHAT_ID = "00000000-0000-4000-8000-000000000001";
const MESSAGE_ID = "10000000-0000-4000-8000-000000000001";

const CHAT: VaultChatRecord = {
  id: CHAT_ID,
  title: "chat",
  created_at: 1,
};

const MESSAGE: VaultMessageRecord = {
  id: MESSAGE_ID,
  chat_id: CHAT_ID,
  role: "user",
  content: "hello",
  timestamp: 2,
};

function locked(): VaultState {
  return vaultReducer(INITIAL_VAULT_STATE, {
    type: "statusReceived",
    status: "locked",
  });
}

function unlockedWithPlaintext(): VaultState {
  let state = vaultReducer(INITIAL_VAULT_STATE, {
    type: "statusReceived",
    status: "unlocked",
  });
  state = vaultReducer(state, { type: "chatsLoadStarted" });
  state = vaultReducer(state, { type: "chatsLoaded", records: [CHAT] });
  state = vaultReducer(state, {
    type: "messagesLoadStarted",
    chatId: CHAT_ID,
    cursor: null,
  });
  return vaultReducer(state, {
    type: "messagesLoaded",
    chatId: CHAT_ID,
    records: [MESSAGE],
    nextCursor: { timestamp: MESSAGE.timestamp, id: MESSAGE.id },
  });
}

test("V-R01 unlock success reaches unlocked", () => {
  let state = vaultReducer(locked(), { type: "unlockStarted" });
  assertEq(state.status, "unlocking", "unlocking status");
  assertEq(state.unlock.phase, "pending", "unlock pending");
  state = vaultReducer(state, { type: "unlockSucceeded", status: "unlocked" });
  assertEq(state.status, "unlocked", "unlocked status");
  assertEq(state.unlock.phase, "idle", "unlock idle");
});

test("V-R02 authentication cancellation remains distinguishable", () => {
  let state = vaultReducer(locked(), { type: "unlockStarted" });
  state = vaultReducer(state, {
    type: "unlockFailed",
    code: "authentication_cancelled",
  });
  assertEq(state.status, "locked", "status");
  assertEq(state.unlock.phase, "failed", "phase");
  assertEq(state.unlock.error, "authentication_cancelled", "code");
});

test("V-R03 authentication failure remains distinguishable", () => {
  let state = vaultReducer(locked(), { type: "unlockStarted" });
  state = vaultReducer(state, {
    type: "unlockFailed",
    code: "authentication_failed",
  });
  assertEq(state.status, "locked", "status");
  assertEq(state.unlock.error, "authentication_failed", "code");
});

test("V-R04 vault quarantine failure becomes quarantined", () => {
  let state = vaultReducer(locked(), { type: "unlockStarted" });
  state = vaultReducer(state, {
    type: "unlockFailed",
    code: "vault_quarantined",
  });
  assertEq(state.status, "quarantined", "status");
  assertEq(state.unlock.error, "vault_quarantined", "code");
});

test("V-R05 recovery-required status is preserved and purges plaintext", () => {
  const state = vaultReducer(unlockedWithPlaintext(), {
    type: "statusReceived",
    status: "recovery_required",
  });
  assertEq(state.status, "recovery_required", "status");
  assertEq(state.chats.items.length, 0, "chats purged");
  assertEq(state.messages.items.length, 0, "messages purged");
});

test("V-R06 lock starts by purging plaintext and completes locked", () => {
  let state = vaultReducer(unlockedWithPlaintext(), { type: "lockStarted" });
  assertEq(state.status, "locking", "locking status");
  assertEq(state.chats.items.length, 0, "chats purged immediately");
  assertEq(state.messages.items.length, 0, "messages purged immediately");
  state = vaultReducer(state, { type: "lockSucceeded" });
  assertEq(state.status, "locked", "locked status");
});

test("V-R07 external quarantine purges plaintext", () => {
  const state = vaultReducer(unlockedWithPlaintext(), {
    type: "statusReceived",
    status: "quarantined",
  });
  assertEq(state.chats.items.length, 0, "chats purged");
  assertEq(state.messages.items.length, 0, "messages purged");
  assertEq(state.save.phase, "idle", "save state purged");
});

test("V-R08 save pending then saved retains the message id", () => {
  let state = unlockedWithPlaintext();
  state = vaultReducer(state, { type: "savePending", messageId: MESSAGE_ID });
  assertEq(state.save.phase, "pending", "pending");
  assertEq(state.save.messageId, MESSAGE_ID, "pending id");
  state = vaultReducer(state, {
    type: "saveSucceeded",
    messageId: MESSAGE_ID,
    record: MESSAGE,
  });
  assertEq(state.save.phase, "saved", "saved");
  assertEq(state.save.messageId, MESSAGE_ID, "saved id");
});

test("V-R09 failed save retries with the exact same message id", () => {
  let state = unlockedWithPlaintext();
  state = vaultReducer(state, { type: "savePending", messageId: MESSAGE_ID });
  state = vaultReducer(state, {
    type: "saveFailed",
    messageId: MESSAGE_ID,
    code: "timeout",
  });
  assertEq(state.save.phase, "failed", "failed");
  assertEq(state.save.messageId, MESSAGE_ID, "failed id");
  state = vaultReducer(state, { type: "saveRetried" });
  assertEq(state.save.phase, "pending", "retry pending");
  assertEq(state.save.messageId, MESSAGE_ID, "retry id unchanged");
  assertOk(state.save.error === null, "retry clears prior error");
});

test("V-R10 stale save completion is an impossible-transition no-op", () => {
  let state = unlockedWithPlaintext();
  state = vaultReducer(state, { type: "savePending", messageId: MESSAGE_ID });
  const next = vaultReducer(state, {
    type: "saveFailed",
    messageId: "20000000-0000-4000-8000-000000000002",
    code: "storage_failed",
  });
  assertOk(next === state, "stale action returns identical state");
});

test("V-R11 repository actions are ignored while locked", () => {
  const state = locked();
  const chats = vaultReducer(state, { type: "chatsLoadStarted" });
  const save = vaultReducer(state, { type: "savePending", messageId: MESSAGE_ID });
  assertOk(chats === state, "chat load ignored");
  assertOk(save === state, "save ignored");
});

test("V-R12 message completion for another chat is ignored", () => {
  let state = vaultReducer(INITIAL_VAULT_STATE, {
    type: "statusReceived",
    status: "unlocked",
  });
  state = vaultReducer(state, {
    type: "messagesLoadStarted",
    chatId: CHAT_ID,
    cursor: null,
  });
  const next = vaultReducer(state, {
    type: "messagesLoaded",
    chatId: "30000000-0000-4000-8000-000000000003",
    records: [MESSAGE],
    nextCursor: null,
  });
  assertOk(next === state, "wrong-chat completion ignored");
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
