import type {
  ContextManifestResponseV1,
  RetrievalManifestV1,
} from "./manifest";

export type ManifestFetchPhase = "loading" | "ready" | "empty" | "error";

export const MANIFEST_FETCH_ERROR_MESSAGE =
  "コンテキストマニフェストの取得または検証に失敗しました";

export interface ManifestFetchState {
  readonly phase: ManifestFetchPhase;
  readonly manifest: RetrievalManifestV1 | null;
  readonly errorMessage: string;
  readonly activeSeq: number;
}

export const INITIAL_MANIFEST_FETCH_STATE: ManifestFetchState = {
  phase: "empty",
  manifest: null,
  errorMessage: "",
  activeSeq: 0,
};

export type ManifestFetchEvent =
  | { kind: "REQUEST_START"; seq: number }
  | {
      kind: "REQUEST_SUCCESS";
      seq: number;
      response: ContextManifestResponseV1;
    }
  | { kind: "REQUEST_FAILURE"; seq: number };

export function reduceManifestFetch(
  state: ManifestFetchState,
  event: ManifestFetchEvent,
): ManifestFetchState {
  if (event.kind === "REQUEST_START") {
    if (event.seq <= state.activeSeq) {
      return state;
    }
    return {
      phase: "loading",
      manifest: null,
      errorMessage: "",
      activeSeq: event.seq,
    };
  }

  if (event.seq !== state.activeSeq) {
    return state;
  }

  if (event.kind === "REQUEST_SUCCESS") {
    if (event.response.reason === "NO_MANIFEST") {
      return {
        phase: "empty",
        manifest: null,
        errorMessage: "",
        activeSeq: state.activeSeq,
      };
    }
    return {
      phase: "ready",
      manifest: event.response.manifest,
      errorMessage: "",
      activeSeq: state.activeSeq,
    };
  }

  return {
    phase: "error",
    manifest: null,
    errorMessage: MANIFEST_FETCH_ERROR_MESSAGE,
    activeSeq: state.activeSeq,
  };
}
