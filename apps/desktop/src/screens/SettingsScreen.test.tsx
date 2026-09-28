import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./SettingsScreen";
import * as tauriApi from "../api/tauri";
import type { Settings } from "../api/types";

const baseSettings: Settings = {
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
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SettingsScreen — Google アカウント", () => {
  it("shows signed-out status and calls google_sign_in when the sign-in button is clicked", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    const signIn = vi.spyOn(tauriApi, "googleSignIn").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText("未サインイン")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    await waitFor(() => expect(signIn).toHaveBeenCalledTimes(1));
  });

  it("shows signed-in status with sign-out and drive-sync buttons", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    const driveSync = vi.spyOn(tauriApi, "driveSyncNow").mockResolvedValue("uploaded");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText("サインイン済み")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "今すぐ Drive 同期" }));
    await waitFor(() => expect(driveSync).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("uploaded")).toBeInTheDocument();
  });

  it("toggling the calendar checkbox and saving includes calendarEnabled", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("サインイン済み");
    fireEvent.click(screen.getByLabelText("Google カレンダーを自動で取り込む"));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ calendarEnabled: true });
  });

  it("disables the sign-in button while in progress and shows the error on rejection", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    let reject!: (e: string) => void;
    vi.spyOn(tauriApi, "googleSignIn").mockReturnValue(
      new Promise<void>((_, r) => {
        reject = r;
      }),
    );
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("未サインイン");
    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    expect(await screen.findByText("処理中…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google でサインイン" })).toBeDisabled();

    reject("sign-in timed out");
    expect(await screen.findByText("sign-in timed out")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google でサインイン" })).toBeEnabled();
  });
});
