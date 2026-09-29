export function extractPackageJsonVersion(jsonText) {
  const data = JSON.parse(jsonText);
  if (typeof data.version !== "string") {
    throw new Error('package.json does not have a string "version" field');
  }
  return data.version;
}

export function extractCargoTomlVersion(tomlText) {
  const lines = tomlText.split("\n");
  let inPackageSection = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (line.startsWith("[")) {
      inPackageSection = line === "[package]";
      continue;
    }
    if (inPackageSection) {
      const match = line.match(/^version\s*=\s*"([^"]+)"/);
      if (match) {
        return match[1];
      }
    }
  }
  throw new Error("Cargo.toml has no [package] version field");
}

export function extractTauriConfVersion(jsonText) {
  const data = JSON.parse(jsonText);
  if (typeof data.version !== "string") {
    throw new Error('tauri.conf.json does not have a string "version" field');
  }
  return data.version;
}

export function checkVersionsMatch({ packageJson, cargoToml, tauriConf }) {
  const versions = {
    "apps/desktop/package.json": packageJson,
    "apps/desktop/src-tauri/Cargo.toml": cargoToml,
    "apps/desktop/src-tauri/tauri.conf.json": tauriConf,
  };
  const unique = new Set(Object.values(versions));
  return { ok: unique.size === 1, versions };
}
