import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as tauriApi from "./api/tauri";
import { checkForUpdate } from "./api/updater";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./api/updater", () => ({
  checkForUpdate: vi.fn().mockResolvedValue({ available: false }),
  installUpdateAndRestart: vi.fn(),
}));

afterEach(() => {
  vi.restoreAllMocks();
});

describe("App", () => {
  it("shows the onboarding screen when setup has not been completed", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockResolvedValue(false);
    render(<App />);
    expect(await screen.findByText("AREITU へようこそ")).toBeInTheDocument();
  });

  it("shows the normal list screen when setup has already been completed", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockResolvedValue(true);
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([]);
    render(<App />);
    expect(await screen.findByText("設定")).toBeInTheDocument();
    expect(screen.queryByText("AREITU へようこそ")).not.toBeInTheDocument();
  });

  it("shows a loading state (not onboarding) while setupCompleted is pending", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockReturnValue(new Promise(() => {}));
    render(<App />);
    expect(screen.getByRole("status")).toHaveTextContent("読み込み中");
    expect(screen.queryByText("AREITU へようこそ")).not.toBeInTheDocument();
  });

  it("falls back to the list screen when setupCompleted rejects", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockRejectedValue(new Error("boom"));
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([]);
    render(<App />);
    expect(await screen.findByText("設定")).toBeInTheDocument();
    expect(screen.queryByText("AREITU へようこそ")).not.toBeInTheDocument();
  });

  it("shows an update banner when a newer version is available", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockResolvedValue(true);
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([]);
    vi.mocked(checkForUpdate).mockResolvedValueOnce({
      available: true,
      // @ts-expect-error テスト用の最小フェイク
      update: {},
      version: "9.9.9",
      notes: null,
    });
    render(<App />);
    expect(await screen.findByText(/9\.9\.9/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "更新して再起動" })).toBeInTheDocument();
  });
});
