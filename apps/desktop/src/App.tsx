import {
  useEffect,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { ConsultTab } from "./components/ConsultTab";
import { ImportTab } from "./components/ImportTab";
import { InterviewTab } from "./components/InterviewTab";
import { MobileChrome } from "./components/MobileChrome";
import { ModelSetupGate } from "./components/ModelSetupGate";
import { PocketBrainPanel } from "./components/PocketBrainPanel";
import { ProbeTab } from "./components/ProbeTab";
import { ProfileTab } from "./components/ProfileTab";
import { RecordTab } from "./components/RecordTab";
import { SettingsTab } from "./components/SettingsTab";
import { TitleBar } from "./components/TitleBar";
import { VaultPanel } from "./components/VaultPanel";
import { engineHealth, engineReady, warmConsultRuntime } from "./lib/engine";
import type { MainTab } from "./lib/types";
import { useForegroundRestore } from "./lib/useForegroundRestore";
import { useIsNarrowViewport } from "./lib/useIsNarrowViewport";
import "./App.css";

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
  const [modelGateDone, setModelGateDone] = useState(false);
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState("Coraxis を起動しています…");
  const [tab, setTab] = useState<MainTab>("record");
  const isNarrow = useIsNarrowViewport();

  // Phase 10: Jetsam / vault-lock → ordered vault→LLM→analytics resync.
  useForegroundRestore(modelGateDone);

  useEffect(() => {
    if (!modelGateDone) return;
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
            // CONSULT 用 LLM/埋め込みをバックグラウンドでウォーム (入力阻害なし)
            void warmConsultRuntime(true).catch(() => {
              /* best-effort */
            });
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
  }, [isNarrow, modelGateDone]);

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

  // Offline setup gate: block main UI until pocket-brain.gguf exists
  // (or pocket-brain feature absent → soft skip inside the gate).
  if (!modelGateDone) {
    return <ModelSetupGate onReady={() => setModelGateDone(true)} />;
  }

  // M20-E: mobile shell is primary — never trap under LoadingScreen.
  // N4: propagate live `ready` (do not hardcode true).
  if (isNarrow) {
    return (
      <div className="shell">
        <TitleBar />
        <MobileChrome statusLine={status} engineReady={ready} />
      </div>
    );
  }

  // N1: PocketBrain + Vault stay mounted for the desktop app lifetime.
  // Loading copy is gated by `ready`; panels are only visually hidden after unlock
  // so GGUF warm / vault watchers are not torn down by the 3s fail-open.
  return (
    <div className="shell">
      <TitleBar />
      <div
        className={
          ready
            ? "desktop-pb-keepalive"
            : "app loading desktop-chrome desktop-pb-keepalive"
        }
        hidden={ready}
        aria-hidden={ready}
      >
        {!ready ? (
          <>
            <h1>Coraxis</h1>
            <p className="status-line">{status}</p>
            <p className="hint">
              初回起動はエンジン展開に 30 秒ほどかかることがあります。
            </p>
          </>
        ) : null}
        <PocketBrainPanel />
        <VaultPanel />
      </div>
      {ready ? (
        <div className="desktop-chrome">
          <header className="topbar">
            <div>
              <h1>Coraxis</h1>
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
      ) : null}
    </div>
  );
}
