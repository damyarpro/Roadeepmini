// `npm run pack`: builds the app with Tauri and copies the installer Tauri buries
// in target/release/bundle/nsis/ into windows/release/, with the name it ships
// under. Used locally and by the release workflow, so both produce exactly the
// same file names.
//
// With no release variables set this is a plain `tauri build` plus the copy.
// Two things are opt-in through the environment (see RELEASING.md):
//   - TAURI_SIGNING_PRIVATE_KEY(_PATH) + ROADEEP_RELEASE_BASE_URL: updater
//     artifacts — the installer's .sig and release/latest.json;
//   - ROADEEP_SIGN_THUMBPRINT or ROADEEP_SIGN_COMMAND: Authenticode signing of
//     the app, the installer and the two relays bundled as resources (Tauri
//     signs the first two only, so the relays are signed here, between
//     `tauri build --no-bundle` and `tauri bundle`).
//
//   node scripts/pack.mjs [--notes "text" | --notes-file path] [--skip-build]

import { spawnSync } from "node:child_process";
import {
  copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  buildLatestJson, ghReleaseCommand, notesFromChangelog, parseGithubBase, planBuild, releaseAssetUrl,
} from "./release-lib.mjs";
import { prepareBundle } from "./stage-local-ai.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const bundleDir = join(root, "target", "release", "bundle", "nsis");
const outDir = join(root, "release");
const RELAYS = ["roadeep-hook.exe", "roadeep-mcp.exe"].map((f) => join(root, "target", "release", f));

function fail(message) {
  console.error(`\n  ${message}\n`);
  process.exit(1);
}

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const plan = planBuild(process.env);
for (const w of plan.warnings) console.warn(`\n  Warning: ${w}`);
if (plan.errors.length > 0) fail(plan.errors.join("\n\n  "));

const { version } = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));

// ── Build ─────────────────────────────────────────────────────────────────────

function tauri(args) {
  const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
  const r = spawnSync(process.execPath, [cli, ...args], {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, ...plan.childEnv },
  });
  if (r.status !== 0) fail(`tauri ${args[0]} failed (exit ${r.status ?? r.signal}).`);
}

/** Program Files (x86)\Windows Kits\10\bin\<newest>\x64\signtool.exe, or ROADEEP_SIGNTOOL. */
function findSigntool() {
  if (process.env.ROADEEP_SIGNTOOL) return process.env.ROADEEP_SIGNTOOL;
  const kits = join(process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)", "Windows Kits", "10", "bin");
  const versions = existsSync(kits) ? readdirSync(kits).filter((d) => /^10\./.test(d)).sort().reverse() : [];
  for (const v of versions) {
    const p = join(kits, v, "x64", "signtool.exe");
    if (existsSync(p)) return p;
  }
  fail("signtool.exe not found. Install the Windows SDK or set ROADEEP_SIGNTOOL to its path.");
}

function signFile(file) {
  const s = plan.signing;
  let r;
  if (s.kind === "thumbprint") {
    r = spawnSync(findSigntool(), [
      "sign", "/fd", "sha256", "/sha1", s.thumbprint, "/tr", s.timestampUrl, "/td", "sha256", file,
    ], { stdio: "inherit" });
  } else {
    // The command comes from the person running the build, like Tauri's own signCommand.
    r = spawnSync(s.command.replaceAll("%1", `"${file}"`), { stdio: "inherit", shell: true });
  }
  if (r.status !== 0) fail(`Signing ${relative(root, file)} failed (exit ${r.status ?? r.signal}).`);
  console.log(`  Signed ${relative(root, file)}`);
}

if (!process.argv.includes("--skip-build")) {
  try {
    await prepareBundle();
  } catch (error) {
    fail(`Offline local AI assets are unavailable: ${error.message}\n  Run node scripts/stage-local-ai.mjs --source <trusted-asset-directory> first.`);
  }
  if (!plan.config) {
    tauri(["build"]);
  } else {
    const configPath = join(root, "target", "pack-config.json");
    mkdirSync(dirname(configPath), { recursive: true });
    writeFileSync(configPath, JSON.stringify(plan.config, null, 2));
    if (plan.signing) {
      tauri(["build", "--no-bundle", "--config", configPath]);
      for (const relay of RELAYS) signFile(relay);
      tauri(["bundle", "--config", configPath]);
    } else {
      tauri(["build", "--config", configPath]);
    }
  }
}

// ── Copy ──────────────────────────────────────────────────────────────────────

let installers = [];
try {
  installers = readdirSync(bundleDir).filter((f) => f.endsWith("-setup.exe"));
} catch {
  // Reported below.
}
if (installers.length === 0) fail(`No installer in ${bundleDir} — the build did not produce one.`);

// Newest wins, in case an older build is still lying around.
const built = installers
  .map((f) => join(bundleDir, f))
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];

mkdirSync(outDir, { recursive: true });
const versionedName = `Roadeep-Windows-${version}-setup.exe`;
const versioned = join(outDir, versionedName);
const rolling = join(outDir, "Roadeep-Windows-setup.exe");
copyFileSync(built, versioned);
copyFileSync(built, rolling);

const mb = (statSync(versioned).size / 1024 / 1024).toFixed(2);
console.log(`\n  Installer ready — ${mb} MB\n`);
console.log(`  ${versioned}`);
console.log(`  ${rolling}\n`);

// ── Updater files ─────────────────────────────────────────────────────────────

if (plan.updaterArtifacts) {
  const sigSource = `${built}.sig`;
  if (!existsSync(sigSource)) fail(`No updater signature next to the installer (${sigSource}).`);
  const signature = readFileSync(sigSource, "utf8");
  const sigOut = `${versioned}.sig`;
  copyFileSync(sigSource, sigOut);

  let notes = arg("--notes");
  if (notes === undefined && arg("--notes-file")) notes = readFileSync(resolve(arg("--notes-file")), "utf8").trim();
  if (notes === undefined) {
    for (const candidate of [join(root, "CHANGELOG.md"), join(root, "..", "CHANGELOG.md")]) {
      if (!existsSync(candidate)) continue;
      notes = notesFromChangelog(readFileSync(candidate, "utf8"), version) ?? undefined;
      if (notes !== undefined) break;
    }
  }
  if (notes === undefined) {
    notes = `Roadeep ${version}`;
    console.warn(`  Warning: no "## ${version}" changelog section and no --notes; using "${notes}".`);
  }

  const latest = buildLatestJson({
    version,
    notes,
    date: new Date(),
    signature,
    url: releaseAssetUrl(plan.baseUrl, version, versionedName),
  });
  const latestPath = join(outDir, "latest.json");
  writeFileSync(latestPath, `${JSON.stringify(latest, null, 2)}\n`);
  const notesPath = join(outDir, `notes-${version}.md`);
  writeFileSync(notesPath, `${notes}\n`);

  console.log(`  ${sigOut}`);
  console.log(`  ${latestPath}`);
  console.log(`  installer URL in latest.json: ${latest.platforms["windows-x86_64"].url}\n`);

  const gh = parseGithubBase(plan.baseUrl, version);
  const rel = (p) => relative(root, p).replaceAll("\\", "/");
  const command = ghReleaseCommand({
    repo: gh?.repo,
    tag: gh?.tag ?? `v${version}`,
    title: `Roadeep ${version}`,
    notesFile: rel(notesPath),
    files: [rel(versioned), rel(sigOut), rel(latestPath)],
  });
  console.log("  To publish (from windows/, not run for you):\n");
  console.log(`    ${command}\n`);
  if (!gh) console.log("  (ROADEEP_RELEASE_BASE_URL is not a GitHub release URL: upload the three files there yourself.)\n");
}
