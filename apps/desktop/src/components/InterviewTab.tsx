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

const MODES: { id: InterviewSurface; label: string; shortLabel: string; hint: string }[] = [
  {
    id: "interview_pocket",
    label: "[ 1on1面接 ]",
    shortLabel: "[ 1on1 ]",
    hint: "技術・人物の1対1面接。企業コンテキストと任意のESを前提に進めます。",
  },
  {
    id: "multistage",
    label: "[ 多段設計 ]",
    shortLabel: "[ 多段 ]",
    hint: "Foundation → Pressure → Debrief → Closed の多段面接。",
  },
  {
    id: "es_pocket",
    label: "[ ES解析 ]",
    shortLabel: "[ ES ]",
    hint: "提出ESを採用責任者視点で添削します。",
  },
  {
    id: "coliseum",
    label: "[ GD闘技 ]",
    shortLabel: "[ GD ]",
    hint: "グループディスカッションの闘技シミュレーション。",
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
          <span className="desktop-only">面接シミュレーター</span>
          <span className="mobile-only">面接</span>
        </h2>
      </div>

      <div
        className="tactical-array interview-mode-array"
        role="tablist"
        aria-label="面接モード"
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

      <p className="hint guide" style={{ margin: "6px 8px" }}>
        {currentMode.hint}
      </p>

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
