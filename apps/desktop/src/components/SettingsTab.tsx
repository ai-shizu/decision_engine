import { useEffect, useState } from "react";
import {
  loadSettings,
  runProfiler,
  saveFixedAttributes,
} from "../lib/engine";
import { defaultBirthday } from "../lib/birthdayUtils";
import type { FixedField, SettingsData } from "../lib/types";
import { BirthdayPicker } from "./BirthdayPicker";

export function SettingsTab() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [attrs, setAttrs] = useState<Record<string, string>>({});
  const [status, setStatus] = useState("");
  const [saveNotice, setSaveNotice] = useState<{ text: string; kind: "success" | "error" } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState("");

  async function fetchSettings() {
    setLoadError("");
    setStatus("");
    try {
      const s = await loadSettings();
      const merged = { ...s.fixed_attributes };
      if (!merged.birthday?.trim()) {
        merged.birthday = defaultBirthday();
      }
      setSettings(s);
      setAttrs(merged);
    } catch (err) {
      const msg = String(err);
      setLoadError(msg);
      setStatus(msg);
    }
  }

  useEffect(() => {
    void fetchSettings();
  }, []);

  function updateAttr(key: string, value: string) {
    setAttrs((prev) => ({ ...prev, [key]: value }));
  }

  async function handleSaveFixed() {
    setBusy(true);
    setStatus("");
    setSaveNotice(null);
    try {
      await saveFixedAttributes(attrs);
      setSaveNotice({ text: "基本情報を保存しました", kind: "success" });
    } catch (err) {
      setSaveNotice({ text: String(err), kind: "error" });
    } finally {
      setBusy(false);
    }
  }

  async function handleProfiler() {
    setBusy(true);
    setStatus("プロファイラ実行中… (数分かかる場合があります)");
    try {
      const res = await runProfiler();
      setStatus(res.message);
      const s = await loadSettings();
      setSettings(s);
      setAttrs({ ...s.fixed_attributes });
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (!settings) {
    return (
      <section className="panel">
        <p className="hint">{loadError ? "設定の読み込みに失敗しました" : "設定を読み込み中…"}</p>
        {loadError && (
          <div className="action-row">
            <button type="button" className="primary" onClick={() => void fetchSettings()}>
              再読み込み
            </button>
          </div>
        )}
        {status && <p className="status-line error-text">{status}</p>}
        {!loadError && (
          <p className="hint">初回はエンジンの準備に数十秒かかることがあります。</p>
        )}
      </section>
    );
  }

  return (
    <section className="panel settings-panel">
      <h2>設定 (SETTINGS)</h2>

      <div className="settings-block">
        <h3>基本情報</h3>
        <p className="hint">手入力の基本情報（profiler では変更されません）</p>
        <div className="fixed-fields">
          {settings.fixed_fields.map((f: FixedField) => (
            <div
              key={f.key}
              className={f.key === "birthday" ? "settings-field-row settings-field-birthday" : "settings-field-row"}
            >
              <label htmlFor={f.key === "birthday" ? undefined : `fixed-${f.key}`}>{f.label}</label>
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
          ))}
        </div>
        <div className="save-block">
          <button type="button" className="primary" disabled={busy} onClick={() => void handleSaveFixed()}>
            基本情報を保存
          </button>
          {saveNotice && (
            <p className={`save-notice ${saveNotice.kind}`}>{saveNotice.text}</p>
          )}
        </div>
      </div>

      <div className="settings-block">
        <h3>自動プロフィール</h3>
        <p className="hint">日記・LINE・相談・家計簿から profiler が抽象化して抽出します（読み取り専用）</p>
        <pre className="profile-box">{settings.profile_summary || "(プロファイル未生成)"}</pre>
        <button type="button" className="secondary" disabled={busy} onClick={() => void handleProfiler()}>
          再分析 (profiler)
        </button>
      </div>

      {status && <p className="status-line">{status}</p>}
    </section>
  );
}
