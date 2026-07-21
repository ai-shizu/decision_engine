/**
 * Phase 14 — dense monospace transcript stream (Arena).
 * Message text is monochrome; no decorative accent on body copy.
 */

export type TranscriptRole = "INTERVIEWER" | "CANDIDATE";

export type TranscriptStage =
  | "FOUNDATION"
  | "PRESSURE"
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
};

function formatTurnId(raw: string): string {
  const m = /^t-?(\d+)$/i.exec(raw.trim());
  if (m) {
    return `T-${m[1].padStart(2, "0")}`;
  }
  return raw.toUpperCase();
}

export function TranscriptStream({ messages }: TranscriptStreamProps) {
  return (
    <div className="tx-stream" aria-label="Interview transcript stream">
      <div className="tx-stream-label">TRANSCRIPT STREAM</div>
      <ul className="tx-stream-list">
        {messages.map((msg) => (
          <li
            key={msg.turnId}
            className={
              msg.role === "INTERVIEWER"
                ? "tx-msg tx-msg-interviewer"
                : "tx-msg tx-msg-candidate"
            }
          >
            <div className="tx-msg-meta">
              <span className="tx-msg-role">
                {msg.role} [{msg.stage}]
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
