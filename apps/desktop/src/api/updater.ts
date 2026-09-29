import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdateCheckResult =
  | { available: false }
  | { available: true; update: Update; version: string; notes: string | null };

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  try {
    const update = await check();
    if (update === null) {
      return { available: false };
    }
    return { available: true, update, version: update.version, notes: update.body ?? null };
  } catch {
    // ネットワークがない、pubkey が未設定/不正、署名検証失敗など。
    // 更新チェックの失敗でアプリの起動自体を妨げないよう、常に available: false として扱う。
    return { available: false };
  }
}

export async function installUpdateAndRestart(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
