import { describe, expect, it, vi } from "vitest";
import * as core from "@tauri-apps/api/core";
import {
  driveSyncNow,
  getSettings,
  googleSignIn,
  googleSignOut,
  googleStatus,
  saveSettings,
  setupCompleted,
} from "./tauri";
import type { Settings } from "./types";

vi.mock("@tauri-apps/api/core", async () => {
  const actual = await vi.importActual<typeof import("@tauri-apps/api/core")>("@tauri-apps/api/core");
  return { ...actual };
});

describe("tauri api layer", () => {
  it("googleSignIn invokes google_sign_in with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    await googleSignIn();
    expect(spy).toHaveBeenCalledWith("google_sign_in");
  });

  it("googleSignOut invokes google_sign_out with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    await googleSignOut();
    expect(spy).toHaveBeenCalledWith("google_sign_out");
  });

  it("googleStatus invokes google_status and returns its result", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue("signed_in");
    await expect(googleStatus()).resolves.toBe("signed_in");
    expect(spy).toHaveBeenCalledWith("google_status");
  });

  it("driveSyncNow invokes drive_sync_now with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue("uploaded");
    await expect(driveSyncNow()).resolves.toBe("uploaded");
    expect(spy).toHaveBeenCalledWith("drive_sync_now");
  });

  it("setupCompleted invokes setup_completed with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(true);
    await expect(setupCompleted()).resolves.toBe(true);
    expect(spy).toHaveBeenCalledWith("setup_completed");
  });

  it("getSettings invokes get_settings with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue({
      watchedDirs: [],
      llmProvider: "none",
      ollamaUrl: "http://localhost:11434",
      ollamaModel: "",
      openaiModel: "gpt-4o-mini",
      geminiModel: "gemini-1.5-flash",
      googlePlacesEnabled: false,
      minConfidence: 0.6,
      pollIntervalMinutes: 30,
      calendarEnabled: false,
      openaiKeyStatus: "not_set",
      geminiKeyStatus: "not_set",
      googlePlacesKeyStatus: "not_set",
    });
    await getSettings();
    expect(spy).toHaveBeenCalledWith("get_settings");
  });

  it("saveSettings invokes save_settings with the settings object under the settings key", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    const input: Settings & { openaiApiKey: string | null; geminiApiKey: string | null; googlePlacesApiKey: string | null } = {
      watchedDirs: ["/photos"],
      llmProvider: "none",
      ollamaUrl: "http://localhost:11434",
      ollamaModel: "",
      openaiModel: "gpt-4o-mini",
      geminiModel: "gemini-1.5-flash",
      googlePlacesEnabled: false,
      minConfidence: 0.6,
      pollIntervalMinutes: 30,
      calendarEnabled: true,
      openaiKeyStatus: "not_set",
      geminiKeyStatus: "not_set",
      googlePlacesKeyStatus: "not_set",
      openaiApiKey: null,
      geminiApiKey: null,
      googlePlacesApiKey: null,
    };
    await saveSettings(input);
    expect(spy).toHaveBeenCalledWith("save_settings", { settings: input });
  });
});
