import { describe, expect, it } from "vitest";
import {
  DEFAULT_TIMESTAMP_URL, buildLatestJson, ghReleaseCommand, notesFromChangelog, parseGithubBase, planBuild,
  pubDate, releaseAssetUrl,
} from "./release-lib.mjs";

const BASE = "https://github.com/acme/roadeep-desktop/releases/download/v{version}";
const THUMB = "0123456789abcdef0123456789abcdef01234567";

describe("planBuild", () => {
  it("changes nothing without release variables", () => {
    const plan = planBuild({ PATH: "x" });
    expect(plan).toMatchObject({ updaterArtifacts: false, signing: null, config: null, errors: [], warnings: [] });
    expect(plan.childEnv).toEqual({});
  });

  it("turns updater artifacts on with a key and a base URL", () => {
    const plan = planBuild({
      TAURI_SIGNING_PRIVATE_KEY: "secret",
      ROADEEP_RELEASE_BASE_URL: `${BASE}/`,
      ROADEEP_UPDATE_URL: "https://github.com/acme/roadeep-desktop/releases/latest/download/latest.json",
      ROADEEP_UPDATE_PUBKEY: "PUBKEY",
    });
    expect(plan.errors).toEqual([]);
    expect(plan.warnings).toEqual([]);
    expect(plan.baseUrl).toBe(BASE);
    expect(plan.config).toEqual({
      bundle: { createUpdaterArtifacts: true },
      plugins: { updater: { pubkey: "PUBKEY" } },
    });
    expect(plan.childEnv).toEqual({});
  });

  it("passes a key path to the CLI the way it reads it", () => {
    const plan = planBuild({ TAURI_SIGNING_PRIVATE_KEY_PATH: "C:\\keys\\roadeep.key", ROADEEP_RELEASE_BASE_URL: BASE });
    expect(plan.updaterArtifacts).toBe(true);
    expect(plan.childEnv).toEqual({ TAURI_SIGNING_PRIVATE_KEY: "C:\\keys\\roadeep.key" });
    expect(plan.warnings[0]).toMatch(/will not look for updates/);
  });

  it("refuses artifacts without a base URL, or with an http one", () => {
    expect(planBuild({ TAURI_SIGNING_PRIVATE_KEY: "k" }).errors[0]).toMatch(/ROADEEP_RELEASE_BASE_URL is missing/);
    expect(planBuild({ TAURI_SIGNING_PRIVATE_KEY: "k", ROADEEP_RELEASE_BASE_URL: "http://x/y" }).errors[0]).toMatch(/https/);
  });

  it("signs with a certificate thumbprint", () => {
    const plan = planBuild({ ROADEEP_SIGN_THUMBPRINT: ` ${THUMB} ` });
    expect(plan.errors).toEqual([]);
    expect(plan.signing).toEqual({ kind: "thumbprint", thumbprint: THUMB.toUpperCase(), timestampUrl: DEFAULT_TIMESTAMP_URL });
    expect(plan.config.bundle.windows).toEqual({
      certificateThumbprint: THUMB.toUpperCase(), digestAlgorithm: "sha256", timestampUrl: DEFAULT_TIMESTAMP_URL, tsp: true,
    });
    expect(plan.updaterArtifacts).toBe(false);
  });

  it("signs with a custom command and combines with updater artifacts", () => {
    const plan = planBuild({
      ROADEEP_SIGN_COMMAND: "trusted-signing-cli -e https://eus.codesigning.azure.net -a acc -c prof %1",
      TAURI_SIGNING_PRIVATE_KEY: "k",
      ROADEEP_RELEASE_BASE_URL: BASE,
    });
    expect(plan.errors).toEqual([]);
    expect(plan.signing.kind).toBe("command");
    expect(plan.config.bundle).toEqual({
      createUpdaterArtifacts: true,
      windows: { signCommand: "trusted-signing-cli -e https://eus.codesigning.azure.net -a acc -c prof %1" },
    });
  });

  it("rejects bad signing settings", () => {
    expect(planBuild({ ROADEEP_SIGN_THUMBPRINT: "abc" }).errors[0]).toMatch(/40 hex/);
    expect(planBuild({ ROADEEP_SIGN_COMMAND: "sign.exe" }).errors[0]).toMatch(/%1/);
    expect(planBuild({ ROADEEP_SIGN_THUMBPRINT: THUMB, ROADEEP_SIGN_COMMAND: "x %1" }).errors[0]).toMatch(/not both/);
  });
});

describe("latest.json", () => {
  const date = new Date("2026-10-02T09:30:15.123Z");

  it("has the updater's format with both Windows keys", () => {
    const url = releaseAssetUrl(BASE, "0.2.0", "Roadeep-Windows-0.2.0-setup.exe");
    expect(url).toBe("https://github.com/acme/roadeep-desktop/releases/download/v0.2.0/Roadeep-Windows-0.2.0-setup.exe");
    const json = buildLatestJson({ version: "0.2.0", notes: "Fixes", date, signature: "c2ln\n", url });
    expect(json).toEqual({
      version: "0.2.0",
      notes: "Fixes",
      pub_date: "2026-10-02T09:30:15Z",
      platforms: {
        "windows-x86_64-nsis": { signature: "c2ln", url },
        "windows-x86_64": { signature: "c2ln", url },
      },
    });
  });

  it("refuses what the updater would reject", () => {
    const ok = { version: "0.2.0", notes: "", date, signature: "s", url: "https://x/y.exe" };
    expect(() => buildLatestJson({ ...ok, version: "next" })).toThrow(/version/);
    expect(() => buildLatestJson({ ...ok, signature: " " })).toThrow(/signature/);
    expect(() => buildLatestJson({ ...ok, url: "http://x/y.exe" })).toThrow(/https/);
  });

  it("formats dates without milliseconds", () => {
    expect(pubDate(date)).toBe("2026-10-02T09:30:15Z");
  });
});

describe("notesFromChangelog", () => {
  const log = "# Changelog\n\n## Unreleased\n\n- soon\n\n## [0.2.0] - 2026-10-02\n\n- Auto-update\n- Fixes\n\n## 0.1.1\n\n- Old\n";

  it("takes the version's section only", () => {
    expect(notesFromChangelog(log, "0.2.0")).toBe("- Auto-update\n- Fixes");
    expect(notesFromChangelog(log, "0.1.1")).toBe("- Old");
    expect(notesFromChangelog("## v0.3.0\r\nA\r\n", "0.3.0")).toBe("A");
  });

  it("returns null when the version has no section", () => {
    expect(notesFromChangelog(log, "0.1.10")).toBeNull();
    expect(notesFromChangelog(log, "9.9.9")).toBeNull();
    expect(notesFromChangelog("## 1.0.0\n\n## 0.9.0", "1.0.0")).toBeNull();
  });
});

describe("publishing", () => {
  it("reads the repository and tag from a GitHub base URL", () => {
    expect(parseGithubBase(BASE, "0.2.0")).toEqual({ repo: "acme/roadeep-desktop", tag: "v0.2.0" });
    expect(parseGithubBase("https://cdn.example.com/roadeep/0.2.0", "0.2.0")).toBeNull();
  });

  it("prints a gh command with every file", () => {
    expect(ghReleaseCommand({
      repo: "acme/roadeep-desktop",
      tag: "v0.2.0",
      title: "Roadeep 0.2.0",
      notesFile: "release/notes-0.2.0.md",
      files: ["release/Roadeep-Windows-0.2.0-setup.exe", "release/Roadeep-Windows-0.2.0-setup.exe.sig", "release/latest.json"],
    })).toBe(
      "gh release create v0.2.0 release/Roadeep-Windows-0.2.0-setup.exe release/Roadeep-Windows-0.2.0-setup.exe.sig " +
        "release/latest.json --repo acme/roadeep-desktop --title \"Roadeep 0.2.0\" --notes-file release/notes-0.2.0.md",
    );
  });
});
