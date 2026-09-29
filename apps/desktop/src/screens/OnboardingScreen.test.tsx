import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as dialog from "@tauri-apps/plugin-dialog";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OnboardingScreen } from "./OnboardingScreen";
import * as tauriApi from "../api/tauri";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

afterEach(() => {
  vi.restoreAllMocks();
  vi.mocked(dialog.open).mockReset();
});

describe("OnboardingScreen", () => {
  it("adds a folder chosen from the native picker to the watched list", async () => {
    vi.mocked(dialog.open).mockResolvedValue(["/Users/me/Pictures"]);
    render(<OnboardingScreen onFinish={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "フォルダを選択" }));
    expect(await screen.findByText("/Users/me/Pictures")).toBeInTheDocument();
  });

  it("skip saves a minimal config and calls onFinish without requiring any input", async () => {
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "スキップ" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ watchedDirs: [], llmProvider: "none", calendarEnabled: false });
    await waitFor(() => expect(onFinish).toHaveBeenCalledTimes(1));
  });

  it("finish saves the chosen folder and llm provider and calls onFinish", async () => {
    vi.mocked(dialog.open).mockResolvedValue(["/Users/me/Pictures"]);
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "フォルダを選択" }));
    await screen.findByText("/Users/me/Pictures");
    fireEvent.click(screen.getByRole("button", { name: "はじめる" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ watchedDirs: ["/Users/me/Pictures"] });
    await waitFor(() => expect(onFinish).toHaveBeenCalledTimes(1));
  });

  it("google sign-in errors are shown but do not block finishing", async () => {
    vi.spyOn(tauriApi, "googleSignIn").mockRejectedValue(new Error("user closed the browser"));
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    expect(await screen.findByText("Error: user closed the browser")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "はじめる" })).not.toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "はじめる" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onFinish).toHaveBeenCalledTimes(1));
  });

  it("sign-in passes the current calendar checkbox state, even when signing in before ticking it", async () => {
    const signIn = vi.spyOn(tauriApi, "googleSignIn").mockResolvedValue(undefined);
    render(<OnboardingScreen onFinish={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    await waitFor(() => expect(signIn).toHaveBeenLastCalledWith(false));

    fireEvent.click(screen.getByLabelText("Google カレンダーを自動で取り込む"));
    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    await waitFor(() => expect(signIn).toHaveBeenLastCalledWith(true));
  });

  it("cancelling the folder picker leaves the list unchanged", async () => {
    vi.mocked(dialog.open).mockResolvedValue(null);
    render(<OnboardingScreen onFinish={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "フォルダを選択" }));
    await waitFor(() => expect(dialog.open).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("listitem")).not.toBeInTheDocument();
  });

  it("shows the save error and does not finish when saving fails", async () => {
    vi.spyOn(tauriApi, "saveSettings").mockRejectedValue(new Error("disk full"));
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "スキップ" }));
    expect(await screen.findByText("Error: disk full")).toBeInTheDocument();
    expect(onFinish).not.toHaveBeenCalled();
  });
});
