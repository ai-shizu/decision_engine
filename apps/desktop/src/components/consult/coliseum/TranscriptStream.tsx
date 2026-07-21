/**
 * Phase 14 — dense monospace transcript stream (Arena).
 * 1:1 interview roles or GD multi-agent roles.
 */

export type TranscriptRole1on1 = "INTERVIEWER" | "CANDIDATE";

export type TranscriptRoleGd =
  | "PARTICIPANT_A"
  | "PARTICIPANT_B"
  | "PARTICIPANT_C"
  | "PARTICIPANT_D"
  | "PARTICIPANT_E"
  | "USER"
  | "FACILITATOR";

export type TranscriptRole = TranscriptRole1on1 | TranscriptRoleGd;

export type TranscriptStage =
  | "FOUNDATION"
  | "PRESSURE"
  | "DISCUSSION"
  | "DEBRIEF"
  | "CLOSED";

export type TranscriptMessage = {
  turnId: string;
  role: TranscriptRole;
  stage: TranscriptStage;
  text: string;
};

export type TranscriptStreamProps = {
  messages: TranscriptMessage[];
  /** When true, treat as multi-agent GD stream (label + CSS branch). */
  multiAgent?: boolean;
};

function formatTurnId(raw: string): string {
  const m = /^t-?(\d+)$/i.exec(raw.trim());
  if (m) {
    return `T-${m[1].padStart(2, "0")}`;
  }
  return raw.toUpperCase();
}

function roleClass(role: TranscriptRole): string {
  if (role === "INTERVIEWER") return "tx-msg tx-msg-interviewer";
  if (role === "CANDIDATE" || role === "USER") return "tx-msg tx-msg-candidate";
  if (role === "FACILITATOR") return "tx-msg tx-msg-facilitator";
  return "tx-msg tx-msg-participant";
}

export function TranscriptStream({
  messages,
  multiAgent = false,
}: TranscriptStreamProps) {
  return (
    <div
      className={multiAgent ? "tx-stream tx-stream-gd" : "tx-stream"}
      aria-label={multiAgent ? "GD multi-agent transcript" : "Interview transcript stream"}
    >
      <div className="tx-stream-label">
        {multiAgent ? "MULTI-AGENT STREAM" : "TRANSCRIPT STREAM"}
      </div>
      <ul className="tx-stream-list">
        {messages.map((msg) => (
          <li key={msg.turnId} className={roleClass(msg.role)} data-role={msg.role}>
            <div className="tx-msg-meta">
              <span className="tx-msg-role">
                [{msg.role}] [{msg.stage}]
              </span>
              <span className="tx-msg-turn">{formatTurnId(msg.turnId)}</span>
            </div>
            <pre className="tx-msg-body">{msg.text}</pre>
          </li>
        ))}
      </ul>
    </div>
  );
}
