import {
  useEffect,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { ConsultTab } from "./components/ConsultTab";
import { ImportTab } from "./components/ImportTab";
import { InterviewTab } from "./components/InterviewTab";
import { MobileChrome } from "./components/MobileChrome";
import { ProbeTab } from "./components/ProbeTab";
import { ProfileTab } from "./components/ProfileTab";
import { RecordTab } from "./components/RecordTab";
import { SettingsTab } from "./components/SettingsTab";
import { TitleBar } from "./components/TitleBar";
import { engineHealth, engineReady } from "./lib/engine";
import type { MainTab } from "./lib/types";
import "./App.css";
// M4 pocket-brain (docs/architecture_blueprint.md §3.9). Mounted on the loading
// screen because that is where the iOS shell sits (engine never becomes ready on
// device). On desktop it shows briefly before the 7-tab UI takes over.
// M20-A: MobileChrome also mounts PocketBrain on ≤768px (CSS-gated).
import { PocketBrainPanel } from "./components/PocketBrainPanel";
import { VaultPanel } from "./components/VaultPanel";

function LoadingScreen({ message }: { message: string }) {
  return (
    <div className="shell">
      <TitleBar />
      <main className="app loading desktop-chrome">
        <h1>PKB</h1>
        <p className="status-line">{message}</p>
        <p className="hint">初回起動はエンジン展開に 30 秒ほどかかることがあります。</p>
        <PocketBrainPanel />
        <VaultPanel />
      </main>
      <MobileChrome statusLine={message} />
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

function tabButtonId(id: MainTab): string {
  return `main-tab-${id}`;
}

function tabPanelId(id: MainTab): string {
  return `main-tabpanel-${id}`;
}

function renderMainTab(id: MainTab) {
  switch (id) {
    case "record":
      return <RecordTab />;
    case "import":
      return <ImportTab />;
    case "consult":
      return <ConsultTab />;
    case "interview":
      return <InterviewTab />;
    case "probe":
      return <ProbeTab />;
    case "profile":
      return <ProfileTab />;
    case "settings":
      return <SettingsTab />;
    default: {
      const _exhaustive: never = id;
      throw new Error(`unreachable main tab: ${_exhaustive as string}`);
    }
  }
}

function focusTabByIndex(index: number): void {
  const len = TABS.length;
  const normalized = ((index % len) + len) % len;
  const target = TABS[normalized];
  if (!target) return;
  document.getElementById(tabButtonId(target.id))?.focus();
}

function handleTabKeyDown(
  event: ReactKeyboardEvent<HTMLButtonElement>,
  index: number,
): void {
  let next: number | null = null;
  switch (event.key) {
    case "ArrowLeft":
      next = index - 1;
      break;
    case "ArrowRight":
      next = index + 1;
      break;
    case "Home":
      next = 0;
      break;
    case "End":
      next = TABS.length - 1;
      break;
    default:
      return;
  }
  event.preventDefault();
  focusTabByIndex(next);
}

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
      <div className="desktop-chrome">
        <header className="topbar">
          <div>
            <h1>PKB</h1>
            <p className="subtitle">{status}</p>
          </div>
          <nav
            className="tabs"
            role="tablist"
            aria-label="メインタブ"
            aria-orientation="horizontal"
          >
            {TABS.map(({ id, label }, index) => (
              <button
                key={id}
                id={tabButtonId(id)}
                type="button"
                role="tab"
                aria-selected={tab === id}
                aria-controls={tabPanelId(id)}
                tabIndex={tab === id ? 0 : -1}
                className={tab === id ? "active" : ""}
                onClick={() => setTab(id)}
                onKeyDown={(event) => handleTabKeyDown(event, index)}
              >
                {label}
              </button>
            ))}
          </nav>
        </header>
        <main className="content">
          {TABS.map(({ id }) => (
            <div
              key={id}
              id={tabPanelId(id)}
              className="main-tab-panel"
              role="tabpanel"
              aria-labelledby={tabButtonId(id)}
              hidden={tab !== id}
              tabIndex={tab === id ? 0 : -1}
            >
              {tab === id && renderMainTab(id)}
            </div>
          ))}
        </main>
      </div>
      <MobileChrome statusLine={status} />
    </div>
  );
}
