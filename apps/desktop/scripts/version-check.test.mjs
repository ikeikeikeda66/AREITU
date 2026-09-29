import { describe, expect, it } from "vitest";
import {
  checkVersionsMatch,
  extractCargoTomlVersion,
  extractPackageJsonVersion,
  extractTauriConfVersion,
} from "./version-check.mjs";

describe("extractPackageJsonVersion", () => {
  it("reads the version field", () => {
    expect(extractPackageJsonVersion('{"name":"desktop","version":"1.2.3"}')).toBe("1.2.3");
  });

  it("throws a clear error when version is missing", () => {
    expect(() => extractPackageJsonVersion('{"name":"desktop"}')).toThrow(
      /does not have a string "version" field/,
    );
  });
});

describe("extractCargoTomlVersion", () => {
  it("reads the version from the [package] section", () => {
    const toml = `[package]\nname = "areitu-desktop"\nversion = "1.2.3"\nedition = "2021"\n`;
    expect(extractCargoTomlVersion(toml)).toBe("1.2.3");
  });

  it("ignores version fields in other sections", () => {
    const toml = `[package]\nname = "areitu-desktop"\nversion = "1.2.3"\n\n[dependencies]\nserde = { version = "1" }\n`;
    expect(extractCargoTomlVersion(toml)).toBe("1.2.3");
  });

  it("throws a clear error when [package] has no version", () => {
    const toml = `[dependencies]\nserde = { version = "1" }\n`;
    expect(() => extractCargoTomlVersion(toml)).toThrow(/no \[package\] version field/);
  });
});

describe("extractTauriConfVersion", () => {
  it("reads the version field", () => {
    expect(extractTauriConfVersion('{"productName":"AREITU","version":"1.2.3"}')).toBe("1.2.3");
  });
});

describe("checkVersionsMatch", () => {
  it("reports ok when all three versions are equal", () => {
    const result = checkVersionsMatch({ packageJson: "1.2.3", cargoToml: "1.2.3", tauriConf: "1.2.3" });
    expect(result.ok).toBe(true);
  });

  it("reports not ok and lists the mismatched files when versions differ", () => {
    const result = checkVersionsMatch({ packageJson: "1.2.3", cargoToml: "1.2.4", tauriConf: "1.2.3" });
    expect(result.ok).toBe(false);
    expect(result.versions["apps/desktop/src-tauri/Cargo.toml"]).toBe("1.2.4");
  });
});
