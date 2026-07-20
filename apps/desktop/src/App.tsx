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
import { useIsNarrowViewport } from "./lib/useIsNarrowViewport";
import "./App.css";
// M4 pocket-brain: desktop LoadingScreen only. Mobile uses MobileChrome (CONSULT
// messenger) and must not stay trapped behind LoadingScreen with engineReady=false.
import { PocketBrainPanel } from "./components/PocketBrainPanel";
import { VaultPanel } from "./components/VaultPanel";

/** Desktop-only boot gate. Mobile never uses this tree (see App). */
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
  const isNarrow = useIsNarrowViewport();

  useEffect(() => {
    let cancelled = false;

    function sleep(ms: number): Promise<void> {
      return new Promise((r) => setTimeout(r, ms));
    }

    /** Desktop: fail-open after budget. Mobile: unlock immediately (no sidecar scare). */
    const ENGINE_READY_BUDGET_MS = 3000;

    async function waitForEngine() {
      // M20-M: iOS/mobile never depends on Python sidecar for shell unlock.
      if (isNarrow) {
        if (!cancelled) {
          setReady(true);
          setStatus("");
        }
        // Soft background upgrade when sidecar eventually answers.
        for (let i = 0; i < 40 && !cancelled; i += 1) {
          try {
            if (await engineReady()) {
              try {
                await engineHealth();
              } catch {
                /* best-effort */
              }
              if (!cancelled) setStatus("準備完了");
              return;
            }
          } catch {
            /* keep soft */
          }
          await sleep(500);
        }
        return;
      }

      const deadline = Date.now() + ENGINE_READY_BUDGET_MS;
      while (!cancelled && Date.now() < deadline) {
        try {
          if (await engineReady()) {
            try {
              await engineHealth();
            } catch {
              /* health is best-effort once ready flips */
            }
            if (!cancelled) {
              setReady(true);
              setStatus("準備完了（完全オフライン）");
            }
            return;
          }
        } catch {
          /* retry until budget */
        }
        if (!cancelled) setStatus("エンジン起動中…");
        await sleep(250);
      }
      if (!cancelled) {
        setReady(true);
        setStatus("準備完了");
      }
    }

    void waitForEngine();
    return () => {
      cancelled = true;
    };
  }, [isNarrow]);

  // SPEC_FOXTROT_UI.md §3.6 — desktop Alt+[1-7] only.
  useEffect(() => {
    if (!ready || isNarrow) return;
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
  }, [ready, isNarrow]);

  // M20-E: mobile shell is primary — never trap under LoadingScreen with
  // engineReady frozen to false. Live `ready` updates Settings/RECORD IPC.
  if (isNarrow) {
    return (
      <div className="shell">
        <TitleBar />
        <MobileChrome statusLine={status} engineReady={true} />
      </div>
    );
  }

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
    </div>
  );
}
