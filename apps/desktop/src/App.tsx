import { useEffect, useState } from "react";
import { ConsultTab } from "./components/ConsultTab";
import { ImportTab } from "./components/ImportTab";
import { InterviewTab } from "./components/InterviewTab";
import { RecordTab } from "./components/RecordTab";
import { SettingsTab } from "./components/SettingsTab";
import { engineHealth, engineReady } from "./lib/engine";
import type { MainTab } from "./lib/types";
import "./App.css";

function LoadingScreen({ message }: { message: string }) {
  return (
    <main className="app loading">
      <h1>PKB</h1>
      <p className="status-line">{message}</p>
      <p className="hint">初回起動はエンジン展開に 30 秒ほどかかることがあります。</p>
    </main>
  );
}

const TABS: { id: MainTab; label: string }[] = [
  { id: "record", label: "RECORD" },
  { id: "import", label: "IMPORT" },
  { id: "consult", label: "CONSULT" },
  { id: "interview", label: "INTERVIEW" },
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

  if (!ready) {
    return <LoadingScreen message={status} />;
  }

  return (
    <div className="shell">
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
        {tab === "settings" && <SettingsTab />}
      </main>
    </div>
  );
}
