#!/usr/bin/env node
// Release guard: fail when plugins.updater.pubkey in tauri.conf.json is missing or
// still the placeholder. A release built with the placeholder ships updater
// artifacts that no installed app can verify, so those users would never update.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const confPath = process.argv[2] ?? join(scriptDir, "..", "src-tauri", "tauri.conf.json");

function fail(message) {
  console.error(`Refusing to release: ${message}`);
  process.exit(1);
}

let conf;
try {
  conf = JSON.parse(readFileSync(confPath, "utf8"));
} catch (e) {
  fail(`cannot read ${confPath} as JSON (${e.message})`);
}

const pubkey = conf?.plugins?.updater?.pubkey;
if (typeof pubkey !== "string" || pubkey.trim() === "") {
  fail(`plugins.updater.pubkey is missing or empty in ${confPath}.`);
}
if (pubkey.includes("REPLACE_WITH")) {
  fail(
    `plugins.updater.pubkey in ${confPath} still contains the placeholder "REPLACE_WITH...". ` +
      "Generate a minisign key pair, put the public key there, and store the private key in the " +
      "TAURI_SIGNING_PRIVATE_KEY secret. Updater artifacts built with the placeholder cannot be " +
      "verified by any installed app.",
  );
}
console.log("Updater pubkey is set.");
