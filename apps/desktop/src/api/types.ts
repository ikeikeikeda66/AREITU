export interface Place {
  id: number;
  name: string;
  visitCount: number;
  lastVisit: string;
}

export interface Visit {
  startedAt: string;
  endedAt: string;
}

export type SortMode = "count" | "recent";

export interface SyncSummary {
  scanned: number;
  scanErrors: string[];
  visitsCreated: number;
  resolveFailed: number;
  calendarSynced: number;
  calendarRemoved: number;
  calendarErrors: string[];
}

export type LlmProvider = "none" | "ollama" | "openai" | "gemini";

export type KeyStatus = "set" | "not_set" | "unavailable";

export interface Settings {
  watchedDirs: string[];
  llmProvider: LlmProvider;
  ollamaUrl: string;
  ollamaModel: string;
  openaiModel: string;
  geminiModel: string;
  googlePlacesEnabled: boolean;
  minConfidence: number;
  pollIntervalMinutes: number;
  calendarEnabled: boolean;
  /** 直近のカレンダー同期のエラーメッセージ。成功していれば null。 */
  calendarLastError: string | null;
  openaiKeyStatus: KeyStatus;
  geminiKeyStatus: KeyStatus;
  googlePlacesKeyStatus: KeyStatus;
}

export interface SaveSettingsInput {
  watchedDirs: string[];
  llmProvider: LlmProvider;
  ollamaUrl: string;
  ollamaModel: string;
  openaiModel: string;
  geminiModel: string;
  googlePlacesEnabled: boolean;
  minConfidence: number;
  pollIntervalMinutes: number;
  calendarEnabled: boolean;
  openaiApiKey: string | null;
  geminiApiKey: string | null;
  googlePlacesApiKey: string | null;
}

export type GoogleStatus = "signed_in" | "signed_out";
export type DriveSyncOutcome = "no_op" | "uploaded" | "downloaded" | string;
