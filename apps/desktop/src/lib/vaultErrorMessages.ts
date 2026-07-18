/** Fixed, sterile copy for the closed M3 vault status and error contracts. */

import type { VaultErrorCode, VaultStatus } from "./parseVault";

const VAULT_STATUS_LABELS = {
  unprovisioned: "未準備",
  locked: "ロック中",
  unlocking: "ロック解除中",
  unlocked: "接続済み",
  locking: "ロック処理中",
  recovery_required: "復旧確認が必要",
  orphaned_key: "鍵の対応先を確認できません",
  quarantined: "隔離中",
  unavailable: "利用不可",
} as const satisfies Record<VaultStatus, string>;

const VAULT_STATUS_DESCRIPTIONS = {
  unprovisioned: "保管庫を利用するにはロック解除を実行してください。",
  locked: "保管庫はロックされています。OS認証を使ってロック解除できます。",
  unlocking: "OS認証の完了を待っています。",
  unlocked: "暗号化保管庫に接続しています。",
  locking: "接続を閉じ、画面上の平文データを消去しています。",
  recovery_required: "保護データの復旧確認が必要なため、操作を停止しています。",
  orphaned_key: "保護鍵とデータの対応を確認できないため、操作を停止しています。",
  quarantined: "データ保護のため、保管庫の操作を停止しています。",
  unavailable: "保管庫を利用できません。時間をおいて再度お試しください。",
} as const satisfies Record<VaultStatus, string>;

const VAULT_ERROR_MESSAGES = {
  locked: "保管庫はロックされています。ロック解除後に操作してください。",
  busy: "保管庫は別の処理を実行中です。完了後にもう一度お試しください。",
  timeout: "保管庫の応答を確認できませんでした。状態を確認してから再度お試しください。",
  authentication_cancelled: "認証がキャンセルされました。必要な場合にもう一度お試しください。",
  authentication_failed: "認証を確認できませんでした。OSの認証状態を確認してください。",
  interaction_not_allowed: "現在の端末状態では認証操作を開始できません。",
  keychain_unavailable: "保護鍵へアクセスできません。端末のセキュリティ状態を確認してください。",
  corrupt_or_wrong_key: "保護データの整合性を確認できないため、操作を停止しました。",
  unsupported_schema: "対応していない保管庫形式のため、データ保護を優先して操作を停止しました。",
  vault_quarantined: "保管庫は隔離されています。データ保護のため操作できません。",
  unavailable: "保管庫を利用できません。時間をおいて再度お試しください。",
  invalid_input: "入力内容を受け付けられませんでした。内容を確認してください。",
  not_found: "対象のデータを確認できませんでした。",
  conflict: "同じ識別子の異なるデータが存在するため、保存を停止しました。",
  storage_failed: "保管庫の処理結果を確認できませんでした。状態を確認してください。",
  os_lock_engaged:
    "端末のロックにより保管庫が保護され、切断されました。ロックを解除して再度お試しください。",
  unknown: "保管庫の処理結果を確認できませんでした。",
} as const satisfies Record<VaultErrorCode, string>;

export function vaultStatusLabel(status: VaultStatus): string {
  return VAULT_STATUS_LABELS[status];
}

export function vaultStatusDescription(status: VaultStatus): string {
  return VAULT_STATUS_DESCRIPTIONS[status];
}

export function vaultErrorMessage(code: VaultErrorCode): string {
  return VAULT_ERROR_MESSAGES[code];
}
