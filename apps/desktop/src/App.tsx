import { useEffect, useState } from "react";
import { ConsultTab } from "./components/ConsultTab";
import { ImportTab } from "./components/ImportTab";
import { InterviewTab } from "./components/InterviewTab";
import { ProbeTab } from "./components/ProbeTab";
import { ProfileTab } from "./components/ProfileTab";
import { RecordTab } from "./components/RecordTab";
import { SettingsTab } from "./components/SettingsTab";
import { TitleBar } from "./components/TitleBar";
import { engineHealth, engineReady } from "./lib/engine";
import type { MainTab } from "./lib/types";
import "./App.css";

function LoadingScreen({ message }: { message: string }) {
  return (
    <div className="shell">
      <TitleBar />
      <main className="app loading">
        <h1>PKB</h1>
        <p className="status-line">{message}</p>
        <p className="hint">初回起動はエンジン展開に 30 秒ほどかかることがあります。</p>
      </main>
    </div>
  );
}

const TABS: { id: MainTab; label: string }[] = [
  { id: "record", label: "RECORD" },
  { id: "import", label: "IMPORT" },
  { id: "consult", label: "CONSULT" },
  { id: "interview", label: "INTERVIEW" },
  { id: "probe", label: "PROBE" },
  { id: "profile", label: "PROFILE" },
  { id: "settings", label: "SETTINGS" },
];

export default function App() {
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState("PKB を起動しています…");
  const [tab, setTab] = useState<MainTab>("record");

  useEffect(() => {
    let cancelled = false;

    async function waitForEngine() {
      for (let i = 0; i < 120 && !cancelled; i++) {
        try {
          if (await engineReady()) {
            await engineHealth();
            if (!cancelled) {
              setReady(true);
              setStatus("準備完了（完全オフライン）");
            }
            return;
          }
        } catch {
          /* retry */
        }
        if (!cancelled) setStatus("エンジン起動中…");
        await new Promise((r) => setTimeout(r, 1000));
      }
      if (!cancelled) setStatus("エンジンの起動に失敗しました。アプリを再起動してください。");
    }

    void waitForEngine();
    return () => {
      cancelled = true;
    };
  }, []);

  // SPEC_FOXTROT_UI.md §3.6 / SPEC_UI_ORPHAN: Alt+[1-6] (PROBE まで) + [1-7] (PROFILE 追加)。
  useEffect(() => {
    if (!ready) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && /^[1-7]$/.test(e.key)) {
        const idx = Number(e.key) - 1;
        const target = TABS[idx];
        if (target) {
          e.preventDefault();
          setTab(target.id);
        }
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [ready]);

  if (!ready) {
    return <LoadingScreen message={status} />;
  }

  return (
    <div className="shell">
      <TitleBar />
      <header className="topbar">
        <div>
          <h1>PKB</h1>
          <p className="subtitle">{status}</p>
        </div>
        <nav className="tabs">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className={tab === id ? "active" : ""}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </nav>
      </header>
      <main className="content">
        {tab === "record" && <RecordTab />}
        {tab === "import" && <ImportTab />}
        {tab === "consult" && <ConsultTab />}
        {tab === "interview" && <InterviewTab />}
        {tab === "probe" && <ProbeTab />}
        {tab === "profile" && <ProfileTab />}
        {tab === "settings" && <SettingsTab />}
      </main>
    </div>
  );
}
