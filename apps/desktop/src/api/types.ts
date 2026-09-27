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
