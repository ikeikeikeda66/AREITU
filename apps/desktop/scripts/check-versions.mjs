#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkVersionsMatch,
  extractCargoTomlVersion,
  extractPackageJsonVersion,
  extractTauriConfVersion,
} from "./version-check.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const desktopRoot = join(scriptDir, "..");

const packageJsonText = readFileSync(join(desktopRoot, "package.json"), "utf8");
const cargoTomlText = readFileSync(join(desktopRoot, "src-tauri", "Cargo.toml"), "utf8");
const tauriConfText = readFileSync(join(desktopRoot, "src-tauri", "tauri.conf.json"), "utf8");

const result = checkVersionsMatch({
  packageJson: extractPackageJsonVersion(packageJsonText),
  cargoToml: extractCargoTomlVersion(cargoTomlText),
  tauriConf: extractTauriConfVersion(tauriConfText),
});

if (!result.ok) {
  console.error(
    "Version mismatch across apps/desktop/package.json, apps/desktop/src-tauri/Cargo.toml, apps/desktop/src-tauri/tauri.conf.json:",
  );
  for (const [file, version] of Object.entries(result.versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

console.log(`Versions match: ${Object.values(result.versions)[0]}`);
