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
}

export type MainTab = "record" | "import" | "consult" | "settings";

export type RecordSubTab = "events" | "finance" | "diary";
