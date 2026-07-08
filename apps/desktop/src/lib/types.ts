export interface RecordEvent {
  time: string;
  title: string;
}

export interface Transaction {
  type: "expense" | "income";
  category: string;
  amount: number;
}

export interface RecordData {
  date: string;
  events: RecordEvent[];
  transactions: Transaction[];
  diary: string;
}

export interface FixedField {
  key: string;
  label: string;
}

export interface SettingsData {
  fixed_fields: FixedField[];
  fixed_attributes: Record<string, string>;
  profile_summary: string;
  apple_calendar_available: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant";
  text: string;
  /** ストリーミング受信中 (chunk 追記対象) の一時メッセージ */
  streaming?: boolean;
}

/** Python エンジンからの中間イベント (Tauri "pkb-engine-event" 経由) */
export interface EngineEvent {
  id?: number;
  event: "status" | "chunk";
  message?: string;
  text?: string;
}

export type MainTab = "record" | "import" | "consult" | "interview" | "settings";

/** INTERVIEW タブのモード (バックエンド consult の mode と一致させる) */
export type InterviewMode = "interview_sim" | "es_review" | "gd_sim";

/** GD 参加者ペルソナ (最大9人)。trait はプリセット名または自由記述 */
export interface GdPersona {
  name: string;
  trait: string;
}

/**
 * F4a (SPEC_FOXTROT_UI.md §7 裁定2): interview_sim コンフィギュレータ。
 * F-13: スプレッド禁止、この3フィールドの明示列挙のみバックエンドへ送る。
 * industry/genre はプリセットID (バックエンドの静的バンクで解決) または
 * 自由記述文字列。ES が存在する場合はバックエンド側で ES 駆動が優先される。
 */
export interface InterviewConfig {
  industry: string;
  genre: string;
  difficulty: "standard" | "hard" | "extreme";
}

/** F4b: 成績表の1軸分の評価 (バックエンドで軸ホワイトリスト・evidence必須を検証済み) */
export interface InterviewReportMetric {
  axis: string;
  score: number;
  evidence: string;
}

/** F4b: 実測レイテンシ (バックエンドが物理量として合成。LLM は書かない) */
export interface InterviewReportLatency {
  median_sec: number;
  max_sec: number;
  n: number;
}

/**
 * F4b (SPEC_FOXTROT_UI.md §7 裁定3): interview_report.v1。
 * W-37: UI はこの構造体を stdio 応答からそのまま受け取るのみで、
 * LLM 出力の JSON.parse を絶対に書かない。
 */
export interface InterviewReport {
  schema: "interview_report.v1";
  date: string;
  config: Partial<InterviewConfig>;
  metrics: InterviewReportMetric[];
  summary: string;
  latency: InterviewReportLatency;
  simulated: true;
}

/** INTERVIEW タブのチャットメッセージ */
export interface InterviewMessage {
  role: "user" | "ai" | "feedback";
  /** GD で AI 発言を話者別に分割した時の話者名 (面接では "面接官") */
  speaker?: string;
  text: string;
  streaming?: boolean;
  /** ユーザー発言に付随する応答時間 (秒) */
  responseTimeSec?: number;
}

export type RecordSubTab = "events" | "finance" | "diary";

/** F2 (SPEC_FOXTROT_UI.md §2.2.1): IMPORT SourceTable 用の軽量 stat */
export interface SourceStat {
  exists: boolean;
  count: number;
  mtime: string | null;
}

/** F2-EXT (SPEC_FOXTROT_UI.md §2.2.2): import.classify の決定論的分類結果 */
export interface ClassifyResult {
  type: "line" | "ics" | "es" | "knowledge" | "reject";
  reasons: string[];
  size: number;
  filename: string;
  /** フロント側でのみ付与 (再読込を避けるため。バックエンド純関数の戻り値には無い) */
  content?: string;
}
