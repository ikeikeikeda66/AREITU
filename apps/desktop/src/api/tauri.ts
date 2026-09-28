import { invoke } from "@tauri-apps/api/core";
import type { Place, SaveSettingsInput, Settings, SortMode, SyncSummary, Visit } from "./types";

export async function listPlaces(sort: SortMode, keyword: string): Promise<Place[]> {
  const trimmed = keyword.trim();
  return invoke<Place[]>("list_places", { sort, keyword: trimmed === "" ? null : trimmed });
}

export async function visitsOf(placeId: number): Promise<Visit[]> {
  return invoke<Visit[]>("visits_of", { placeId });
}

export async function renamePlace(placeId: number, name: string): Promise<number> {
  return invoke<number>("rename_place", { placeId, name });
}

export async function syncNow(): Promise<SyncSummary> {
  return invoke<SyncSummary>("sync_now");
}

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function saveSettings(settings: SaveSettingsInput): Promise<void> {
  return invoke<void>("save_settings", { settings });
}
