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
}

export type LlmProvider = "none" | "ollama" | "openai" | "gemini";

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
  hasOpenaiKey: boolean;
  hasGeminiKey: boolean;
  hasGooglePlacesKey: boolean;
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
  openaiApiKey: string | null;
  geminiApiKey: string | null;
  googlePlacesApiKey: string | null;
}
