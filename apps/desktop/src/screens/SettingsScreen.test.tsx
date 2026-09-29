import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as dialog from "@tauri-apps/plugin-dialog";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./SettingsScreen";
import * as tauriApi from "../api/tauri";
import type { Settings } from "../api/types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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
  calendarLastError: null,
  openaiKeyStatus: "not_set",
  geminiKeyStatus: "not_set",
  googlePlacesKeyStatus: "not_set",
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.mocked(dialog.open).mockReset();
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
    expect(signIn).toHaveBeenCalledWith(false);
  });

  it("signing in with the calendar box ticked (before saving) requests calendar access", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    const signIn = vi.spyOn(tauriApi, "googleSignIn").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("未サインイン");
    fireEvent.click(screen.getByLabelText("Google カレンダーを自動で取り込む"));
    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    await waitFor(() => expect(signIn).toHaveBeenCalledWith(true));
  });

  it("when signed in with the calendar box ticked, a grant button re-consents with calendar=true", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    const signIn = vi.spyOn(tauriApi, "googleSignIn").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("サインイン済み");
    expect(screen.queryByRole("button", { name: "カレンダーへのアクセスを許可" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Google カレンダーを自動で取り込む"));
    fireEvent.click(screen.getByRole("button", { name: "カレンダーへのアクセスを許可" }));
    await waitFor(() => expect(signIn).toHaveBeenCalledWith(true));
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

describe("SettingsScreen — カレンダー同期エラー", () => {
  it("shows nothing about errors when the last calendar sync succeeded", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue({ ...baseSettings, calendarEnabled: true });
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("サインイン済み");
    expect(screen.queryByText(/直近のカレンダー同期/)).not.toBeInTheDocument();
  });

  it("shows the last calendar error with a re-consent hint when access was not authorized", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue({
      ...baseSettings,
      calendarEnabled: true,
      calendarLastError: "calendar not authorized (HTTP 401/403): calendar.readonly scope is missing",
    });
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText(/calendar not authorized/)).toBeInTheDocument();
    expect(screen.getByText(/「カレンダーへのアクセスを許可」/)).toBeInTheDocument();
  });

  it("shows other calendar errors without the re-consent hint", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue({
      ...baseSettings,
      calendarEnabled: true,
      calendarLastError: "http: timeout",
    });
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText(/http: timeout/)).toBeInTheDocument();
    expect(screen.queryByText(/「カレンダーへのアクセスを許可」/)).not.toBeInTheDocument();
  });
});

describe("SettingsScreen — 読み込み失敗", () => {
  it("shows the error and the sign-in button when the Google status call fails", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockRejectedValue("AREITU_GOOGLE_CLIENT_ID is not set at build time");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText(/AREITU_GOOGLE_CLIENT_ID is not set/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google でサインイン" })).toBeEnabled();
    expect(screen.queryByText("状態を確認しています…")).not.toBeInTheDocument();
  });

  it("shows an error when loading the settings fails", async () => {
    vi.spyOn(tauriApi, "getSettings").mockRejectedValue("settings unavailable");
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText(/settings unavailable/)).toBeInTheDocument();
  });
});

describe("SettingsScreen — Google タイムライン取り込み", () => {
  const importButton = () => screen.findByRole("button", { name: "Google タイムラインを取り込む" });

  it("opens a file picker and shows how many visits were imported", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.mocked(dialog.open).mockResolvedValue("/Users/me/Timeline.json");
    const importFile = vi.spyOn(tauriApi, "importTimelineFile").mockResolvedValue(5);
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await importButton());
    await waitFor(() => expect(importFile).toHaveBeenCalledWith("/Users/me/Timeline.json"));
    expect(await screen.findByText("5 件の訪問を取り込みました")).toBeInTheDocument();
  });

  it("does nothing when the file picker is cancelled", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.mocked(dialog.open).mockResolvedValue(null);
    const importFile = vi.spyOn(tauriApi, "importTimelineFile");
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await importButton());
    await waitFor(() => expect(dialog.open).toHaveBeenCalledTimes(1));
    expect(importFile).not.toHaveBeenCalled();
    expect(screen.queryByText(/取り込みました/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google タイムラインを取り込む" })).toBeEnabled();
  });

  it("shows the error message when the import fails", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.mocked(dialog.open).mockResolvedValue("/Users/me/Timeline.json");
    vi.spyOn(tauriApi, "importTimelineFile").mockRejectedValue("invalid timeline file");
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await importButton());
    expect(await screen.findByText("invalid timeline file")).toBeInTheDocument();
    expect(screen.queryByText(/取り込みました/)).not.toBeInTheDocument();
  });

  it("disables the button while importing", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.mocked(dialog.open).mockResolvedValue("/Users/me/Timeline.json");
    let resolve!: (n: number) => void;
    vi.spyOn(tauriApi, "importTimelineFile").mockReturnValue(
      new Promise<number>((r) => {
        resolve = r;
      }),
    );
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await importButton());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Google タイムラインを取り込む" })).toBeDisabled(),
    );
    resolve(2);
    expect(await screen.findByText("2 件の訪問を取り込みました")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Google タイムラインを取り込む" })).toBeEnabled();
  });
});
