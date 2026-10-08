import { describe, expect, it } from "vitest";
import {
  bumpCargoLock, bumpCargoToml, bumpJsonVersion, bumpPackageLock, currentVersions, validVersion,
} from "./version-lib.mjs";

const PACKAGE = '{\n  "name": "roadeep-windows",\n  "version": "0.1.1",\n  "devDependencies": {\n    "vite": "^6"\n  }\n}\n';
const TAURI = '{\n  "productName": "Roadeep",\n  "version": "0.1.1",\n  "plugins": { "x": { "version": "9.9.9" } },\n  "bundle": { "targets": ["nsis"] }\n}\n';
const CARGO = '[workspace]\nmembers = ["src-tauri"]\n\n[workspace.dependencies]\nversion = "1"\n\n[workspace.package]\nedition = "2021"\nversion = "0.1.1"\n\n[profile.release]\nopt-level = "s"\n';
const LOCK = '[[package]]\nname = "cookie"\nversion = "0.18.1"\n\n[[package]]\nname = "roadeep"\nversion = "0.1.1"\ndependencies = []\n\n[[package]]\nname = "roadeep-hook"\nversion = "0.1.1"\n\n[[package]]\nname = "roadeep-mcp"\nversion = "0.1.1"\n';

describe("validVersion", () => {
  it("accepts plain X.Y.Z only", () => {
    for (const v of ["0.2.0", "1.0.0", "10.20.300"]) expect(validVersion(v)).toBe(true);
    for (const v of ["", "1.0", "1.0.0.0", "01.0.0", "1.0.0-beta", "v1.0.0", " 1.0.0", undefined]) {
      expect(validVersion(v)).toBe(false);
    }
  });
});

describe("bumps", () => {
  it("changes only the top-level version in JSON files, keeping the formatting", () => {
    expect(bumpJsonVersion(PACKAGE, "0.2.0")).toBe(PACKAGE.replace('"version": "0.1.1"', '"version": "0.2.0"'));
    const tauri = bumpJsonVersion(TAURI, "0.2.0");
    expect(tauri).toContain('"version": "0.2.0"');
    expect(tauri).toContain('{ "version": "9.9.9" }');
    expect(tauri).toContain('"targets": ["nsis"]');
    expect(() => bumpJsonVersion("{}", "0.2.0")).toThrow();
  });

  it("updates the lock file's own entries only", () => {
    const lock = JSON.stringify({
      name: "roadeep-windows", version: "0.1.1", lockfileVersion: 3,
      packages: { "": { name: "roadeep-windows", version: "0.1.1" }, "node_modules/vite": { version: "6.0.0" } },
    }, null, 2) + "\n";
    const out = JSON.parse(bumpPackageLock(lock, "0.2.0"));
    expect(out.version).toBe("0.2.0");
    expect(out.packages[""].version).toBe("0.2.0");
    expect(out.packages["node_modules/vite"].version).toBe("6.0.0");
    expect(bumpPackageLock(lock, "0.2.0").endsWith("}\n")).toBe(true);
    expect(bumpPackageLock(lock.replace(/\n/g, "\r\n"), "0.2.0")).toContain('"version": "0.2.0",\r\n');
  });

  it("changes the [workspace.package] version and nothing else in Cargo.toml", () => {
    const out = bumpCargoToml(CARGO, "0.2.0");
    expect(out).toBe(CARGO.replace('edition = "2021"\nversion = "0.1.1"', 'edition = "2021"\nversion = "0.2.0"'));
    expect(out).toContain('[workspace.dependencies]\nversion = "1"');
    expect(() => bumpCargoToml("[package]\nversion = \"1.0.0\"\n", "0.2.0")).toThrow();
  });

  it("changes the workspace crates in Cargo.lock", () => {
    const out = bumpCargoLock(LOCK, ["roadeep", "roadeep-hook", "roadeep-mcp"], "0.2.0");
    expect(out.match(/version = "0\.2\.0"/g)).toHaveLength(3);
    expect(out).toContain('name = "cookie"\nversion = "0.18.1"');
    expect(bumpCargoLock(LOCK.replace(/\n/g, "\r\n"), ["roadeep"], "0.2.0")).toContain('name = "roadeep"\r\nversion = "0.2.0"');
    expect(() => bumpCargoLock(LOCK, ["roadeep", "missing"], "0.2.0")).toThrow(/missing/);
  });

  it("reports the current versions", () => {
    expect(currentVersions({ packageJson: PACKAGE, tauriConf: TAURI, cargoToml: CARGO })).toEqual({
      packageJson: "0.1.1", tauriConf: "0.1.1", cargoToml: "0.1.1",
    });
  });
});
