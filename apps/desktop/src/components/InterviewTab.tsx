import { useState } from "react";
import { emptyCompanyFacts } from "../lib/interviewStage";
import type { CompanyFacts } from "../lib/pocketBrain/types";
import { useCompanyFactsEnrichment } from "../lib/useCompanyFactsEnrichment";
import { useInterviewEsBase } from "../lib/useInterviewEsBase";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { ColiseumRoot } from "./consult/coliseum";
import { CompanyFactsForm } from "./interview/CompanyFactsForm";
import { EsReviewPanel } from "./interview/EsReviewPanel";
import { InterviewEsBaseForm } from "./interview/InterviewEsBaseForm";
import { InterviewPocketPanel } from "./interview/InterviewPocketPanel";
import { MultistageInterviewPanel } from "./interview/MultistageInterviewPanel";

// M18-E: Interview tab is Coraxis-only (Python consult dual-stack removed).
type InterviewSurface = "interview_pocket" | "multistage" | "es_pocket" | "coliseum";

/** Hardware HUD mode indicators — top-tier selection simulators. */
const MODES: { id: InterviewSurface; label: string; shortLabel: string; hint: string }[] = [
  {
    id: "interview_pocket",
    label: "[ 1ON1_TECH ]",
    shortLabel: "[ 1ON1 ]",
    hint: "1ON1_TECH: 技術/人物面接ストリーム (企業ファクト + 任意 ES ベース)。",
  },
  {
    id: "multistage",
    label: "[ SYS_DESIGN ]",
    shortLabel: "[ SYS_DS ]",
    hint: "SYS_DESIGN: Foundation → Pressure → Debrief → Closed 多段プロンプト。",
  },
  {
    id: "es_pocket",
    label: "[ DOC_SCAN ]",
    shortLabel: "[ DOC ]",
    hint: "DOC_SCAN: ES/書類解析 — 採用責任者ストリーム添削。",
  },
  {
    id: "coliseum",
    label: "[ ARENA_GD ]",
    shortLabel: "[ ARENA ]",
    hint: "ARENA_GD: Inner Coliseum グループディスカッション闘技場。",
  },
];

export function InterviewTab() {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<InterviewSurface>("interview_pocket");
  const [sharedFacts, setSharedFacts] = useState<CompanyFacts>(() => emptyCompanyFacts());
  const esBase = useInterviewEsBase();

  const patchSharedFacts = (patch: Partial<CompanyFacts>) => {
    setSharedFacts((prev) => ({ ...prev, ...patch }));
  };
  const {
    researching: sharedResearching,
    provenanceLabel: sharedProvenance,
  } = useCompanyFactsEnrichment(sharedFacts, patchSharedFacts, isNarrow);

  const currentMode = MODES.find((m) => m.id === surface)!;

  function switchSurface(next: InterviewSurface) {
    if (next === surface) return;
    setSurface(next);
  }

  return (
    <section className="panel interview-panel magi-rack">
      <div className="magi-mod-head consult-header">
        <h2>
          <span className="desktop-only">
            INTERVIEW <span className="term-tag term-tag--info">[ SIM_RACK ]</span>
          </span>
          <span className="mobile-only">
            <span className="term-tag term-tag--info">[ INTERVIEW ]</span>
          </span>
        </h2>
        <span className="micro-tel">MODE_LOCK · HUD</span>
      </div>

      <div
        className="tactical-array interview-mode-array"
        role="tablist"
        aria-label="面接稼働モード"
      >
        {MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            role="tab"
            aria-selected={surface === m.id}
            className={surface === m.id ? "active" : ""}
            onClick={() => switchSurface(m.id)}
            title={m.hint}
          >
            <span className="desktop-only">{m.label}</span>
            <span className="mobile-only">{m.shortLabel}</span>
          </button>
        ))}
      </div>

      <div className="magi-mod-foot">
        <span className="micro-tel">{currentMode.hint}</span>
        <span className="micro-tel">SYS.NOMINAL</span>
      </div>

      {isNarrow && surface === "interview_pocket" && (
        <div className="interview-shared-context">
          <InterviewEsBaseForm
            esId={esBase.esId}
            esText={esBase.esText}
            items={esBase.items}
            onEsIdChange={(id) => {
              void esBase.onEsIdChange(id);
            }}
            onEsTextChange={esBase.onEsTextChange}
          />
          <CompanyFactsForm
            facts={sharedFacts}
            onPatch={patchSharedFacts}
            researching={sharedResearching}
            provenanceLabel={sharedProvenance}
          />
        </div>
      )}

      {isNarrow && surface !== "interview_pocket" && surface !== "coliseum" && (
        <div className="interview-shared-context">
          <CompanyFactsForm
            facts={sharedFacts}
            onPatch={patchSharedFacts}
            researching={sharedResearching}
            provenanceLabel={sharedProvenance}
          />
        </div>
      )}

      {surface === "interview_pocket" && (
        <InterviewPocketPanel
          sharedFacts={isNarrow ? sharedFacts : undefined}
          onSharedFactsPatch={isNarrow ? patchSharedFacts : undefined}
          hideEmbeddedFactsForm={isNarrow}
          esText={esBase.esText}
          esBaseSlot={
            isNarrow ? null : (
              <InterviewEsBaseForm
                esId={esBase.esId}
                esText={esBase.esText}
                items={esBase.items}
                onEsIdChange={(id) => {
                  void esBase.onEsIdChange(id);
                }}
                onEsTextChange={esBase.onEsTextChange}
              />
            )
          }
        />
      )}
      {surface === "multistage" && (
        <MultistageInterviewPanel
          sharedFacts={isNarrow ? sharedFacts : undefined}
          onSharedFactsPatch={isNarrow ? patchSharedFacts : undefined}
          hideEmbeddedFactsForm={isNarrow}
        />
      )}
      {surface === "es_pocket" && (
        <EsReviewPanel
          sharedFacts={isNarrow ? sharedFacts : undefined}
          onSharedFactsPatch={isNarrow ? patchSharedFacts : undefined}
          hideEmbeddedFactsForm={isNarrow}
        />
      )}
      {surface === "coliseum" && <ColiseumRoot />}
    </section>
  );
}
