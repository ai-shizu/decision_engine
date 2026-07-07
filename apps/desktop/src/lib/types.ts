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
