import { useEffect, useState } from "react";
import {
  engineReady as checkEngineReady,
  getKnowledgeResearchPolicy,
  loadSettings,
  saveFixedAttributes,
  setKnowledgeResearchPolicy,
} from "../lib/engine";
import { defaultBirthday } from "../lib/birthdayUtils";
import {
  localSettingsShell,
  readLocalFixedAttributes,
  writeLocalFixedAttributes,
} from "../lib/settingsLocalCache";
import type { FixedField, SettingsData } from "../lib/types";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { BirthdayPicker } from "./BirthdayPicker";
import { Toggle } from "./Toggle";

/** Desktop: short probe then fail-open. Mobile uses local-first (no scare). */
const ENGINE_READY_BUDGET_MS = 3000;
const SETTINGS_LOAD_TIMEOUT_MS = 3000;

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timeout")), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err: unknown) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

export function SettingsTab({
  engineReady = true,
  onOpenProfile,
}: {
  engineReady?: boolean;
  /** Mobile: jump to PROFILE surface (single source of analysis). */
  onOpenProfile?: () => void;
}) {
  const isNarrow = useIsNarrowViewport();
  const [settings, setSettings] = useState<SettingsData | null>(() =>
    isNarrow ? localSettingsShell(readLocalFixedAttributes()) : null,
  );
  const [attrs, setAttrs] = useState<Record<string, string>>(() => {
    if (!isNarrow) return {};
    const local = readLocalFixedAttributes();
    const shell = localSettingsShell(local);
    return { ...shell.fixed_attributes };
  });
  const [status, setStatus] = useState("");
  const [statusKind, setStatusKind] = useState<"info" | "error">("info");
  const [saveNotice, setSaveNotice] = useState<{ text: string; kind: "success" | "error" } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [knowledgeResearchEnabled, setKnowledgeResearchEnabled] = useState(false);
  const [policyBusy, setPolicyBusy] = useState(false);
  const [waitingEngine, setWaitingEngine] = useState(false);

  /// Read the persisted NetworkPolicy. Independent of the Python engine so it
  /// still runs on iOS (see the call site in `fetchSettings`).
  async function loadKnowledgePolicy(): Promise<void> {
    try {
      const policy = await withTimeout(getKnowledgeResearchPolicy(), 1500);
      setKnowledgeResearchEnabled(policy.enabled);
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic (原則3)
      console.error("[SettingsTab] knowledge policy load failed:", err);
    }
  }

  async function probeReady(budgetMs: number): Promise<boolean> {
    if (engineReady) return true;
    const deadline = Date.now() + budgetMs;
    while (Date.now() < deadline) {
      try {
        if (await withTimeout(checkEngineReady(), 500)) return true;
      } catch {
        /* keep probing */
      }
      await sleep(250);
    }
    try {
      return await withTimeout(checkEngineReady(), 500);
    } catch {
      return false;
    }
  }

  function applySettings(s: SettingsData) {
    const merged = { ...s.fixed_attributes };
    if (!merged.birthday?.trim()) {
      merged.birthday = defaultBirthday();
    }
    setSettings(s);
    setAttrs(merged);
  }

  async function fetchSettings(opts?: { skipWait?: boolean }) {
    setLoadError("");
    setStatus("");
    setStatusKind("info");

    if (isNarrow) {
      // Instant local shell — never block or scare on mobile.
      applySettings(localSettingsShell(readLocalFixedAttributes()));
      // NetworkPolicy is a pure-Rust command (`knowledge_policy_get`) and works on
      // iOS, where `loadSettings()` (a Python-engine proxy) can never succeed —
      // there is no sidecar on mobile by design. Nesting the policy read inside
      // the settings try meant it was never reached on device, so the toggle was
      // stuck at its initial `false` no matter what was persisted (2026-07-25
      // device E2E). Read it independently, before and regardless of the engine.
      await loadKnowledgePolicy();
      try {
        const s = await withTimeout(loadSettings(), SETTINGS_LOAD_TIMEOUT_MS);
        applySettings(s);
        writeLocalFixedAttributes({ ...s.fixed_attributes });
      } catch (err) {
        // eslint-disable-next-line no-console -- intentional diagnostic (原則3)
        console.error("[SettingsTab] settings load failed (mobile shell):", err);
        /* stay on local shell — seamless */
      }
      return;
    }

    setWaitingEngine(true);
    const ready = opts?.skipWait
      ? engineReady || (await checkEngineReady().catch(() => false))
      : await probeReady(ENGINE_READY_BUDGET_MS);
    setWaitingEngine(false);

    try {
      const s = await withTimeout(loadSettings(), SETTINGS_LOAD_TIMEOUT_MS);
      applySettings(s);
      // Keep the local cache in sync with the engine (mirrors the mobile
      // branch above) so on-device CONSULT sees current Vault-saved basics
      // even when the user hasn't re-hit Save this session.
      writeLocalFixedAttributes({ ...s.fixed_attributes });
      await loadKnowledgePolicy();
      if (!ready) {
        setStatusKind("info");
        setStatus("一部の保存機能は後から有効になります。");
      }
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic (原則3)
      console.error("[SettingsTab] settings load failed:", err);
      applySettings(localSettingsShell(readLocalFixedAttributes()));
      setLoadError("");
      setStatusKind("info");
      setStatus("設定を端末に保持した状態で表示しています。「再読込」で同期できます。");
    }
  }

  useEffect(() => {
    void fetchSettings();
  }, [engineReady, isNarrow]);

  function updateAttr(key: string, value: string) {
    setAttrs((prev) => {
      const next = { ...prev, [key]: value };
      if (isNarrow) writeLocalFixedAttributes(next);
      return next;
    });
  }

  async function handleSaveFixed() {
    setBusy(true);
    setStatus("");
    setStatusKind("info");
    setSaveNotice(null);
    writeLocalFixedAttributes(attrs);
    try {
      await saveFixedAttributes(attrs);
      setSaveNotice({ text: "基本情報を保存しました", kind: "success" });
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic (原則3)
      console.error("[SettingsTab] saveFixedAttributes failed:", err);
      if (isNarrow) {
        setSaveNotice({ text: "基本情報を端末に保存しました", kind: "success" });
      } else {
        setSaveNotice({ text: uiErrorMessage("SETTINGS_SAVE"), kind: "error" });
      }
    } finally {
      setBusy(false);
    }
  }

  async function handleKnowledgePolicyToggle(enabled: boolean) {
    setPolicyBusy(true);
    try {
      const policy = await setKnowledgeResearchPolicy(enabled);
      setKnowledgeResearchEnabled(policy.enabled);
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic (原則3).
      // This exact catch swallowed the 2026-07-25 device failure: the toggle
      // appeared inert with nothing in the console to act on.
      console.error("[SettingsTab] knowledge policy save failed:", err);
      setStatusKind("error");
      setStatus(uiErrorMessage("SETTINGS_SAVE"));
    } finally {
      setPolicyBusy(false);
    }
  }

  if (!settings) {
    return (
      <section className="panel">
        <p className="hint">
          {waitingEngine ? "設定を読み込み中…" : loadError ? "" : "設定を読み込み中…"}
        </p>
        {loadError && (
          <>
            <p className="status-line error-text" role="alert">
              {loadError}
            </p>
            <div className="action-row">
              <button type="button" className="primary" onClick={() => void fetchSettings()}>
                再試行
              </button>
            </div>
          </>
        )}
      </section>
    );
  }

  return (
    <section className="panel settings-panel">
      <h2>
        <span className="desktop-only">
          SETTINGS <span className="term-tag term-tag--info">[ CONFIG ]</span>
        </span>
        <span className="mobile-only">
          <span className="term-tag term-tag--info">[ 設定 ]</span>
        </span>
      </h2>
      <div className="ascii-sep" role="separator">
        --- CONTROLS ---
      </div>
      <div className="action-row">
        <button type="button" className="ghost" onClick={() => void fetchSettings({ skipWait: true })}>
          再読込
        </button>
      </div>

      <div className="settings-block">
        <h3>基本情報</h3>
        <p className="hint">手入力の基本情報（自動分析では変更されません）</p>
        <div className="settings-list">
          {settings.fixed_fields.map((f: FixedField) => (
            <div
              key={f.key}
              className={
                f.key === "birthday"
                  ? "settings-row settings-row-birthday"
                  : "settings-row"
              }
            >
              <label
                htmlFor={f.key === "birthday" ? undefined : `fixed-${f.key}`}
                className="settings-row-label"
              >
                {f.label}
              </label>
              <div className="settings-row-value">
                {f.key === "birthday" ? (
                  <BirthdayPicker
                    value={attrs.birthday ?? defaultBirthday()}
                    onChange={(v) => updateAttr("birthday", v)}
                    disabled={busy}
                  />
                ) : (
                  <input
                    id={`fixed-${f.key}`}
                    value={attrs[f.key] ?? ""}
                    onChange={(e) => updateAttr(f.key, e.target.value)}
                    disabled={busy}
                  />
                )}
              </div>
            </div>
          ))}
        </div>
        <div className="save-block">
          <button type="button" className="primary" disabled={busy} onClick={() => void handleSaveFixed()}>
            基本情報を保存
          </button>
          {saveNotice && (
            <p
              className={`save-notice ${saveNotice.kind}${saveNotice.kind === "error" ? " error-text" : ""}`}
              role={saveNotice.kind === "error" ? "alert" : undefined}
            >
              {saveNotice.text}
            </p>
          )}
        </div>
      </div>

      <div className="settings-block">
        <h3>外部知識</h3>
        <p className="hint">
          同意後、相談送信時に Wikipedia 検索で知識を補強します（オフライン検証パイプライン経由）。
        </p>
        <div className="settings-list">
          <div className="settings-row">
            <label htmlFor="settings-knowledge-research" className="settings-row-label">
              外部ネットワーク検索による知識補強を許可する
            </label>
            <div className="settings-row-value">
              <Toggle
                id="settings-knowledge-research"
                checked={knowledgeResearchEnabled}
                disabled={policyBusy}
                onChange={(v) => void handleKnowledgePolicyToggle(v)}
              />
            </div>
          </div>
        </div>
      </div>

      {/* Auto profile lives on PROFILE tab — avoid duplicate Surfaces (M20-M). */}
      <div className="settings-block">
        <h3>自己分析について</h3>
        <p className="hint">
          日記・行動データからの深い分析結果は、プロフィール画面にまとめています。
        </p>
        {onOpenProfile ? (
          <div className="action-row">
            <button type="button" className="secondary" onClick={onOpenProfile}>
              プロフィールを開く
            </button>
          </div>
        ) : (
          <p className="hint desktop-only">PROFILE タブでギャップ分析・指標を確認できます。</p>
        )}
      </div>

      <details className="settings-advanced">
        <summary>高度な連携・詳細設定</summary>
        <div className="settings-list">
          <div className="settings-row">
            <label htmlFor="settings-apple-calendar" className="settings-row-label">
              iPhoneカレンダー（予定）の読み込み連携
            </label>
            <div className="settings-row-value">
              <Toggle
                id="settings-apple-calendar"
                checked={settings.apple_calendar_available}
                disabled
              />
            </div>
          </div>
        </div>
      </details>

      {status && (
        <p
          className={`status-line${statusKind === "error" ? " error-text" : ""}`}
          role={statusKind === "error" ? "alert" : "status"}
        >
          {status}
        </p>
      )}
    </section>
  );
}
