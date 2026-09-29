import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}));

describe("checkForUpdate", () => {
  it("returns available: false when no update is found", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    vi.mocked(check).mockResolvedValue(null);
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: false });
  });

  it("returns available: true with version and notes when an update is found", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    const fakeUpdate = { version: "1.2.3", body: "バグ修正", downloadAndInstall: vi.fn() };
    // @ts-expect-error テスト用の最小フェイク（Update 型の全フィールドは持たない）
    vi.mocked(check).mockResolvedValue(fakeUpdate);
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: true, update: fakeUpdate, version: "1.2.3", notes: "バグ修正" });
  });

  it("returns available: false when check() throws (e.g. no network, invalid pubkey)", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    vi.mocked(check).mockRejectedValue(new Error("boom"));
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: false });
  });
});

describe("installUpdateAndRestart", () => {
  it("downloads, installs, and relaunches", async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    const download = vi.fn().mockResolvedValue(undefined);
    const fakeUpdate = { downloadAndInstall: download };
    const { installUpdateAndRestart } = await import("./updater");

    // @ts-expect-error テスト用の最小フェイク
    await installUpdateAndRestart(fakeUpdate);

    expect(download).toHaveBeenCalledTimes(1);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });
});
