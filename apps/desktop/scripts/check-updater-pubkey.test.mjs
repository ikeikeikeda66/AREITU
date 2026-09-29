import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const script = join(dirname(fileURLToPath(import.meta.url)), "check-updater-pubkey.mjs");

function run(conf) {
  const dir = mkdtempSync(join(tmpdir(), "updater-pubkey-"));
  const path = join(dir, "tauri.conf.json");
  writeFileSync(path, typeof conf === "string" ? conf : JSON.stringify(conf));
  return spawnSync(process.execPath, [script, path], { encoding: "utf8" });
}

describe("check-updater-pubkey", () => {
  it("exits non-zero with a clear message when the pubkey is still the placeholder", () => {
    const r = run({
      plugins: { updater: { pubkey: "REPLACE_WITH_OWNER_GENERATED_MINISIGN_PUBLIC_KEY_FROM_STEP_1" } },
    });
    expect(r.status).not.toBe(0);
    expect(r.stderr).toContain("REPLACE_WITH");
    expect(r.stderr).toContain("plugins.updater.pubkey");
  });

  it("exits 0 for a real-looking minisign public key", () => {
    const r = run({
      plugins: {
        updater: {
          pubkey:
            "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEyMzQ1Njc4OUFCQ0RFRjAKUldRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQQ==",
        },
      },
    });
    expect(r.status).toBe(0);
  });

  it("fails when the pubkey is missing or empty", () => {
    expect(run({ plugins: { updater: {} } }).status).not.toBe(0);
    expect(run({ plugins: { updater: { pubkey: "  " } } }).status).not.toBe(0);
    expect(run({}).status).not.toBe(0);
  });

  it("fails on unreadable or invalid config", () => {
    expect(run("not json").status).not.toBe(0);
  });
});
