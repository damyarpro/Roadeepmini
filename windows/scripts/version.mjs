// `npm run version -- 0.2.0`: sets the app version everywhere it lives, in one go
// (see scripts/version-lib.mjs for the list). Commits and tags nothing.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  bumpCargoLock, bumpCargoToml, bumpJsonVersion, bumpPackageLock, currentVersions, validVersion,
} from "./version-lib.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WORKSPACE_CRATES = ["roadeep", "roadeep-hook", "roadeep-mcp"];

const version = (process.argv[2] ?? "").replace(/^v/i, "");
if (!validVersion(version)) {
  console.error("Usage: npm run version -- X.Y.Z   (e.g. npm run version -- 0.2.0)");
  process.exit(1);
}

const files = {
  packageJson: join(root, "package.json"),
  packageLock: join(root, "package-lock.json"),
  tauriConf: join(root, "src-tauri", "tauri.conf.json"),
  cargoToml: join(root, "Cargo.toml"),
  cargoLock: join(root, "Cargo.lock"),
};
const text = Object.fromEntries(Object.entries(files).map(([k, p]) => [k, readFileSync(p, "utf8")]));

const before = currentVersions(text);
const distinct = new Set(Object.values(before));
if (distinct.size > 1) {
  console.warn(`  Versions disagreed before the bump: ${JSON.stringify(before)}`);
}

// Compute everything first, so a file that cannot be bumped leaves all of them untouched.
let next;
try {
  next = {
    packageJson: bumpJsonVersion(text.packageJson, version),
    packageLock: bumpPackageLock(text.packageLock, version),
    tauriConf: bumpJsonVersion(text.tauriConf, version),
    cargoToml: bumpCargoToml(text.cargoToml, version),
    cargoLock: bumpCargoLock(text.cargoLock, WORKSPACE_CRATES, version),
  };
} catch (err) {
  console.error(`  Nothing changed: ${err.message}`);
  process.exit(1);
}

for (const [key, path] of Object.entries(files)) {
  if (next[key] !== text[key]) writeFileSync(path, next[key]);
}

console.log(`\n  Version set to ${version} (was ${[...distinct].join(" / ")}) in:`);
for (const path of Object.values(files)) console.log(`    ${path}`);
console.log("\n  Next: add a \"## " + version + "\" section to the changelog, commit, then build the release (RELEASING.md).\n");
