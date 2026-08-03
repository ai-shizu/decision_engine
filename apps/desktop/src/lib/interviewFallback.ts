//! Interview-only sterile fallback mapping (Finding 13 — via UI_ERROR_SPECS).

import { uiErrorMessage } from "./uiErrorMessages";

export interface InterviewFallback {
  /** 面接の文脈に丸め込んだ表示文言。 */
  readonly message: string;
  /** 直前の発言を再送してよいか。 */
  readonly resendable: boolean;
}

function rawToString(rawError: unknown): string {
  if (rawError == null) return "";
  if (typeof rawError === "string") return rawError;
  if (rawError instanceof Error) return rawError.message || String(rawError);
  try {
    return String(rawError);
  } catch (_coerceErr) {
    return "";
  }
}

export function interviewFallbackFor(rawError: unknown): InterviewFallback {
  const hay = rawToString(rawError).toLowerCase();

  if (hay.includes("model_not_loaded")) {
    return {
      message: uiErrorMessage("INTERVIEW_FALLBACK_MODEL_COLD"),
      resendable: true,
    };
  }
  if (hay.includes("vault_locked")) {
    return {
      message: uiErrorMessage("INTERVIEW_FALLBACK_VAULT_LOCKED"),
      resendable: false,
    };
  }
  if (hay.includes("prompt exceeds context budget")) {
    return {
      message: uiErrorMessage("INTERVIEW_FALLBACK_TOO_LONG"),
      resendable: true,
    };
  }
  if (hay.includes("cancelled")) {
    return {
      message: "発言が取り消されました。",
      resendable: true,
    };
  }
  return {
    message: uiErrorMessage("INTERVIEW_FALLBACK_THINKING"),
    resendable: true,
  };
}
