import { useId, useMemo, useState, type CSSProperties } from "react";
import type {
  CandidateStatus,
  ContextLane,
  RetrievalCandidateV1,
  RetrievalManifestV1,
  SourceType,
} from "../lib/manifest";

export interface ContextObservatoryProps {
  manifest: RetrievalManifestV1;
  preview?: boolean;
}

const PREVIEW_LABEL = "STATIC PREVIEW / NO IPC";
const EMPTY_FILTER_MESSAGE = "NO CANDIDATES MATCH FILTER";
const NULL_DISPLAY = "N/A";

const LANE_COLORS: Record<ContextLane, string> = {
  CURRENT: "var(--accent)",
  RECENT_TRANSCRIPT: "var(--ok)",
  WORKING_MEMORY: "var(--text-muted)",
  RETRIEVED_EVIDENCE: "var(--ok-strong)",
};

const SOURCE_TYPE_ORDER: readonly SourceType[] = [
  "CURRENT_QUERY",
  "TRANSCRIPT_TURN",
  "MEMORY_ATOM",
  "KNOWLEDGE_DOCUMENT",
];

type StatusFilter = "ALL" | CandidateStatus;
type LaneFilter = "ALL" | ContextLane;

const STATUS_FILTERS: readonly StatusFilter[] = [
  "ALL",
  "ACCEPTED",
  "ACCEPTED_TRUNCATED",
  "REJECTED",
  "DEDUPLICATED",
];

const LANE_FILTERS: readonly LaneFilter[] = [
  "ALL",
  "CURRENT",
  "RECENT_TRANSCRIPT",
  "WORKING_MEMORY",
  "RETRIEVED_EVIDENCE",
];

const rootStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s4)",
  width: "100%",
  padding: "var(--s3)",
  backgroundColor: "var(--bg-raised)",
  border: "thin solid var(--border)",
  borderRadius: "var(--radius-s)",
  overflowWrap: "anywhere",
};

const sectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s2)",
  width: "100%",
};

const sectionHeadingStyle: CSSProperties = {
  margin: 0,
  fontSize: "0.75rem",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  letterSpacing: 0,
};

const previewLabelStyle: CSSProperties = {
  margin: 0,
  fontSize: "0.75rem",
  fontFamily: "var(--font-mono)",
  color: "var(--accent)",
  letterSpacing: 0,
};

const summaryGridStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(12rem, 1fr))",
  gap: "var(--s2)",
  width: "100%",
};

const summaryItemStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s1)",
  minWidth: 0,
};

const labelStyle: CSSProperties = {
  fontSize: "0.75rem",
  color: "var(--text-muted)",
  letterSpacing: 0,
};

const valueStyle: CSSProperties = {
  fontSize: "0.875rem",
  fontFamily: "var(--font-mono)",
  color: "var(--text)",
  overflowWrap: "anywhere",
  letterSpacing: 0,
};

const breakdownListStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s1)",
  margin: 0,
  padding: 0,
  listStyle: "none",
  width: "100%",
};

const breakdownRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "space-between",
  gap: "var(--s2)",
  fontSize: "0.875rem",
  letterSpacing: 0,
};

const filterGroupStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s2)",
  width: "100%",
};

const filterRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--s2)",
  width: "100%",
};

const ledgerListStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s2)",
  width: "100%",
  margin: 0,
  padding: 0,
  listStyle: "none",
};

const ledgerRowStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(10rem, 1fr))",
  gap: "var(--s2)",
  width: "100%",
  padding: "var(--s2)",
  backgroundColor: "var(--bg-deep)",
  border: "thin solid var(--border)",
  borderRadius: "var(--radius-s)",
  overflowWrap: "anywhere",
};

const ledgerFieldStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s1)",
  minWidth: 0,
};

const emptyStateStyle: CSSProperties = {
  margin: 0,
  fontSize: "0.875rem",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  letterSpacing: 0,
};

function formatPercent(numerator: number, denominator: number): string {
  return `${((numerator / denominator) * 100).toFixed(2)}%`;
}

function formatNullDisplay<T>(value: T | null): string | T {
  return value === null ? NULL_DISPLAY : value;
}

function filterButtonStyle(active: boolean): CSSProperties {
  return {
    padding: "var(--s1) var(--s2)",
    fontSize: "0.75rem",
    fontFamily: "var(--font-mono)",
    color: active ? "var(--text)" : "var(--text-muted)",
    backgroundColor: active ? "var(--bg-selected)" : "var(--bg-raised)",
    border: active ? "thin solid var(--accent)" : "thin solid var(--border)",
    borderRadius: "var(--radius-s)",
    cursor: "pointer",
    letterSpacing: 0,
  };
}

function ContextSummary({
  manifest,
  preview,
}: {
  manifest: RetrievalManifestV1;
  preview: boolean;
}) {
  return (
    <section style={sectionStyle} aria-label="Context summary">
      <h2 style={sectionHeadingStyle}>CONTEXT SUMMARY</h2>
      {preview ? <p style={previewLabelStyle}>{PREVIEW_LABEL}</p> : null}
      <div style={summaryGridStyle}>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>manifest_id</span>
          <span style={valueStyle}>{manifest.manifest_id}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>policy_version</span>
          <span style={valueStyle}>{manifest.policy_version}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>prompt_version</span>
          <span style={valueStyle}>{manifest.prompt_version}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>used_chars</span>
          <span style={valueStyle}>{manifest.used_chars}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>total_budget_chars</span>
          <span style={valueStyle}>{manifest.total_budget_chars}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>formatting_overhead_chars</span>
          <span style={valueStyle}>{manifest.formatting_overhead_chars}</span>
        </div>
        <div style={summaryItemStyle}>
          <span style={labelStyle}>candidates.length</span>
          <span style={valueStyle}>{manifest.candidates.length}</span>
        </div>
      </div>
    </section>
  );
}

function ContextBudgetMeter({ manifest }: { manifest: RetrievalManifestV1 }) {
  const titleId = useId();
  const descId = useId();

  const totalBudget = manifest.total_budget_chars;
  const usedChars = manifest.used_chars;
  const freeChars = totalBudget - usedChars;
  const usedPercent = (usedChars / totalBudget) * 100;
  const freePercent = (freeChars / totalBudget) * 100;

  let cumulativeX = 0;
  const laneRects = manifest.lane_usage.map((lane) => {
    const laneWidth = (lane.used_chars / totalBudget) * 100;
    const rect = {
      lane: lane.lane,
      x: cumulativeX,
      width: laneWidth,
      usedChars: lane.used_chars,
    };
    cumulativeX += laneWidth;
    return rect;
  });

  const descParts = [
    `used_chars ${usedChars}`,
    `total_budget_chars ${totalBudget}`,
    `used_percent ${usedPercent.toFixed(2)}%`,
    `free_chars ${freeChars}`,
    `free_percent ${freePercent.toFixed(2)}%`,
    ...manifest.lane_usage.map(
      (lane) => `${lane.lane} used_chars ${lane.used_chars}`,
    ),
  ];

  return (
    <section style={sectionStyle} aria-label="Context budget meter">
      <h2 style={sectionHeadingStyle}>CONTEXT BUDGET METER</h2>
      <svg
        viewBox="0 0 100 8"
        width="100%"
        role="img"
        aria-labelledby={`${titleId} ${descId}`}
      >
        <title id={titleId}>Context budget lane usage meter</title>
        <desc id={descId}>{descParts.join("; ")}</desc>
        <rect x={0} y={0} width={100} height={8} fill="var(--border)" />
        {laneRects.map((rect) => (
          <rect
            key={rect.lane}
            x={rect.x}
            y={0}
            width={rect.width}
            height={8}
            fill={LANE_COLORS[rect.lane]}
          />
        ))}
      </svg>
      <ul style={breakdownListStyle}>
        {manifest.lane_usage.map((lane) => (
          <li key={lane.lane} style={breakdownRowStyle}>
            <span style={{ color: "var(--text)" }}>{lane.lane}</span>
            <span style={{ color: "var(--text-muted)" }}>
              {lane.used_chars} chars (
              {formatPercent(lane.used_chars, totalBudget)})
            </span>
          </li>
        ))}
        <li style={breakdownRowStyle}>
          <span style={{ color: "var(--text-muted)" }}>FREE</span>
          <span style={{ color: "var(--text-muted)" }}>
            {freeChars} chars ({formatPercent(freeChars, totalBudget)})
          </span>
        </li>
      </ul>
    </section>
  );
}

function SourceTypeBreakdown({ manifest }: { manifest: RetrievalManifestV1 }) {
  const totalBudget = manifest.total_budget_chars;

  const sourceChars = useMemo(() => {
    const totals: Record<SourceType, number> = {
      CURRENT_QUERY: 0,
      TRANSCRIPT_TURN: 0,
      MEMORY_ATOM: 0,
      KNOWLEDGE_DOCUMENT: 0,
    };
    for (const candidate of manifest.candidates) {
      totals[candidate.source_type] += candidate.included_chars;
    }
    return totals;
  }, [manifest.candidates]);

  return (
    <section style={sectionStyle} aria-label="Source type breakdown">
      <h2 style={sectionHeadingStyle}>SOURCE TYPE BREAKDOWN</h2>
      <ul style={breakdownListStyle}>
        {SOURCE_TYPE_ORDER.map((sourceType) => (
          <li key={sourceType} style={breakdownRowStyle}>
            <span style={{ color: "var(--text)" }}>{sourceType}</span>
            <span style={{ color: "var(--text-muted)" }}>
              {sourceChars[sourceType]} chars (
              {formatPercent(sourceChars[sourceType], totalBudget)})
            </span>
          </li>
        ))}
        <li style={breakdownRowStyle}>
          <span style={{ color: "var(--text-muted)" }}>FORMATTING</span>
          <span style={{ color: "var(--text-muted)" }}>
            {manifest.formatting_overhead_chars} chars (
            {formatPercent(manifest.formatting_overhead_chars, totalBudget)})
          </span>
        </li>
      </ul>
    </section>
  );
}

function CandidateFilters({
  statusFilter,
  laneFilter,
  onStatusFilterChange,
  onLaneFilterChange,
}: {
  statusFilter: StatusFilter;
  laneFilter: LaneFilter;
  onStatusFilterChange: (value: StatusFilter) => void;
  onLaneFilterChange: (value: LaneFilter) => void;
}) {
  return (
    <section style={sectionStyle} aria-label="Candidate filters">
      <h2 style={sectionHeadingStyle}>CANDIDATE FILTERS</h2>
      <div style={filterGroupStyle}>
        <div style={filterRowStyle} role="group" aria-label="Status filter">
          {STATUS_FILTERS.map((filter) => {
            const active = statusFilter === filter;
            return (
              <button
                key={filter}
                type="button"
                aria-pressed={active}
                aria-label={`Status filter ${filter}`}
                style={filterButtonStyle(active)}
                onClick={() => onStatusFilterChange(filter)}
              >
                {filter}
              </button>
            );
          })}
        </div>
        <div style={filterRowStyle} role="group" aria-label="Lane filter">
          {LANE_FILTERS.map((filter) => {
            const active = laneFilter === filter;
            return (
              <button
                key={filter}
                type="button"
                aria-pressed={active}
                aria-label={`Lane filter ${filter}`}
                style={filterButtonStyle(active)}
                onClick={() => onLaneFilterChange(filter)}
              >
                {filter}
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function CandidateLedgerRow({ candidate }: { candidate: RetrievalCandidateV1 }) {
  const fields: { label: string; value: string | number }[] = [
    { label: "candidate_id", value: candidate.candidate_id },
    { label: "source_type", value: candidate.source_type },
    { label: "lane", value: candidate.lane },
    { label: "status", value: candidate.status },
    { label: "reason_code", value: candidate.reason_code },
    { label: "char_count", value: candidate.char_count },
    { label: "included_chars", value: candidate.included_chars },
    {
      label: "selection_rank",
      value: formatNullDisplay(candidate.selection_rank),
    },
    { label: "source_index", value: formatNullDisplay(candidate.source_index) },
    { label: "memory_kind", value: formatNullDisplay(candidate.memory_kind) },
  ];

  return (
    <li style={ledgerRowStyle}>
      {fields.map((field) => (
        <div key={field.label} style={ledgerFieldStyle}>
          <span style={labelStyle}>{field.label}</span>
          <span style={valueStyle}>{field.value}</span>
        </div>
      ))}
    </li>
  );
}

function CandidateLedger({
  candidates,
}: {
  candidates: readonly RetrievalCandidateV1[];
}) {
  if (candidates.length === 0) {
    return (
      <section style={sectionStyle} aria-label="Candidate ledger">
        <h2 style={sectionHeadingStyle}>CANDIDATE LEDGER</h2>
        <p style={emptyStateStyle}>{EMPTY_FILTER_MESSAGE}</p>
      </section>
    );
  }

  return (
    <section style={sectionStyle} aria-label="Candidate ledger">
      <h2 style={sectionHeadingStyle}>CANDIDATE LEDGER</h2>
      <ul style={ledgerListStyle}>
        {candidates.map((candidate) => (
          <CandidateLedgerRow
            key={candidate.candidate_id}
            candidate={candidate}
          />
        ))}
      </ul>
    </section>
  );
}

export function ContextObservatory({
  manifest,
  preview = false,
}: ContextObservatoryProps) {
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("ALL");
  const [laneFilter, setLaneFilter] = useState<LaneFilter>("ALL");

  const filteredCandidates = useMemo(() => {
    return manifest.candidates.filter((candidate) => {
      const statusMatch =
        statusFilter === "ALL" || candidate.status === statusFilter;
      const laneMatch =
        laneFilter === "ALL" || candidate.lane === laneFilter;
      return statusMatch && laneMatch;
    });
  }, [manifest.candidates, statusFilter, laneFilter]);

  return (
    <div style={rootStyle}>
      <ContextSummary manifest={manifest} preview={preview} />
      <ContextBudgetMeter manifest={manifest} />
      <SourceTypeBreakdown manifest={manifest} />
      <CandidateFilters
        statusFilter={statusFilter}
        laneFilter={laneFilter}
        onStatusFilterChange={setStatusFilter}
        onLaneFilterChange={setLaneFilter}
      />
      <CandidateLedger candidates={filteredCandidates} />
    </div>
  );
}

export const CONTEXT_OBSERVATORY_PREVIEW_MANIFEST = {
  schema: "retrieval_manifest.v1",
  manifest_id: "cccccccccccccccccccccccccccccccc",
  session_id: "dddddddddddddddddddddddddddddddd",
  transcript_version: 6,
  query_hash: "11111111111111111111111111111111",
  context_hash: "22222222222222222222222222222222",
  policy_version: "retrieval_policy.v1",
  prompt_version: "pv1",
  model_hash: "",
  total_budget_chars: 12000,
  used_chars: 2756,
  formatting_overhead_chars: 176,
  candidates: [
    {
      candidate_id: "CURRENT:11111111111111111111111111111111",
      document_id: "11111111111111111111111111111111",
      content_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      source_type: "CURRENT_QUERY",
      lane: "CURRENT",
      char_count: 450,
      included_chars: 450,
      status: "ACCEPTED",
      reason_code: "ACCEPTED_REQUIRED_CURRENT",
      selection_rank: null,
      source_index: null,
      speaker_alias: null,
      memory_kind: null,
    },
    {
      candidate_id: "CURRENT:22222222222222222222222222222222",
      document_id: "22222222222222222222222222222222",
      content_hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      source_type: "TRANSCRIPT_TURN",
      lane: "CURRENT",
      char_count: 600,
      included_chars: 380,
      status: "ACCEPTED_TRUNCATED",
      reason_code: "ACCEPTED_TRUNCATED_TO_BUDGET",
      selection_rank: 0,
      source_index: 5,
      speaker_alias: null,
      memory_kind: null,
    },
    {
      candidate_id: "RECENT_TRANSCRIPT:33333333333333333333333333333333",
      document_id: "33333333333333333333333333333333",
      content_hash: "cccccccccccccccccccccccccccccccc",
      source_type: "TRANSCRIPT_TURN",
      lane: "RECENT_TRANSCRIPT",
      char_count: 820,
      included_chars: 820,
      status: "ACCEPTED",
      reason_code: "ACCEPTED_WITHIN_BUDGET",
      selection_rank: 1,
      source_index: 3,
      speaker_alias: null,
      memory_kind: null,
    },
    {
      candidate_id: "RECENT_TRANSCRIPT:44444444444444444444444444444444",
      document_id: "44444444444444444444444444444444",
      content_hash: "dddddddddddddddddddddddddddddddd",
      source_type: "TRANSCRIPT_TURN",
      lane: "RECENT_TRANSCRIPT",
      char_count: 950,
      included_chars: 0,
      status: "REJECTED",
      reason_code: "REJECTED_BUDGET_LIMIT",
      selection_rank: 2,
      source_index: 4,
      speaker_alias: null,
      memory_kind: null,
    },
    {
      candidate_id: "WORKING_MEMORY:55555555555555555555555555555555",
      document_id: "55555555555555555555555555555555",
      content_hash: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
      source_type: "MEMORY_ATOM",
      lane: "WORKING_MEMORY",
      char_count: 620,
      included_chars: 620,
      status: "ACCEPTED",
      reason_code: "ACCEPTED_WITHIN_BUDGET",
      selection_rank: 0,
      source_index: 2,
      speaker_alias: null,
      memory_kind: "goal",
    },
    {
      candidate_id: "WORKING_MEMORY:66666666666666666666666666666666",
      document_id: "66666666666666666666666666666666",
      content_hash: "ffffffffffffffffffffffffffffffff",
      source_type: "MEMORY_ATOM",
      lane: "WORKING_MEMORY",
      char_count: 410,
      included_chars: 0,
      status: "DEDUPLICATED",
      reason_code: "DEDUPLICATED_HIGHER_LANE",
      selection_rank: 1,
      source_index: 1,
      speaker_alias: null,
      memory_kind: "claim",
    },
    {
      candidate_id: "RETRIEVED_EVIDENCE:77777777777777777777777777777777",
      document_id: "77777777777777777777777777777777",
      content_hash: "10101010101010101010101010101010",
      source_type: "MEMORY_ATOM",
      lane: "RETRIEVED_EVIDENCE",
      char_count: 310,
      included_chars: 310,
      status: "ACCEPTED",
      reason_code: "ACCEPTED_WITHIN_BUDGET",
      selection_rank: 0,
      source_index: 0,
      speaker_alias: null,
      memory_kind: "datum",
    },
    {
      candidate_id: "RETRIEVED_EVIDENCE:88888888888888888888888888888888",
      document_id: "88888888888888888888888888888888",
      content_hash: "12121212121212121212121212121212",
      source_type: "KNOWLEDGE_DOCUMENT",
      lane: "RETRIEVED_EVIDENCE",
      char_count: 1200,
      included_chars: 0,
      status: "REJECTED",
      reason_code: "REJECTED_LOW_PRIORITY",
      selection_rank: 1,
      source_index: null,
      speaker_alias: null,
      memory_kind: null,
    },
  ],
  lane_usage: [
    {
      lane: "CURRENT",
      budget_chars: 2400,
      used_chars: 875,
      formatting_chars: 45,
      accepted_count: 2,
      rejected_count: 0,
      deduplicated_count: 0,
    },
    {
      lane: "RECENT_TRANSCRIPT",
      budget_chars: 3600,
      used_chars: 872,
      formatting_chars: 52,
      accepted_count: 1,
      rejected_count: 1,
      deduplicated_count: 0,
    },
    {
      lane: "WORKING_MEMORY",
      budget_chars: 4000,
      used_chars: 658,
      formatting_chars: 38,
      accepted_count: 1,
      rejected_count: 0,
      deduplicated_count: 1,
    },
    {
      lane: "RETRIEVED_EVIDENCE",
      budget_chars: 2000,
      used_chars: 351,
      formatting_chars: 41,
      accepted_count: 1,
      rejected_count: 1,
      deduplicated_count: 0,
    },
  ],
} satisfies RetrievalManifestV1;

export function ContextObservatoryPreview() {
  return (
    <ContextObservatory
      manifest={CONTEXT_OBSERVATORY_PREVIEW_MANIFEST}
      preview
    />
  );
}
