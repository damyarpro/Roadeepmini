// Pure helpers for scripts/pack.mjs: what to build from the environment, the
// updater's latest.json, release notes and the `gh release create` line. No I/O
// here, so scripts/release-lib.test.mjs can cover all of it.

export const DEFAULT_TIMESTAMP_URL = "http://timestamp.digicert.com";

const set = (v) => typeof v === "string" && v.trim() !== "";

/**
 * Reads the release environment and decides how `npm run pack` builds.
 * With none of the variables set the result is `{ config: null }` and pack
 * runs a plain `tauri build`, exactly as before.
 */
export function planBuild(env) {
  const errors = [];
  const warnings = [];
  const config = {};
  const childEnv = {};

  // ── Updater artifacts (signed with the updater key, not a certificate) ──
  const keyInline = set(env.TAURI_SIGNING_PRIVATE_KEY);
  const keyPath = set(env.TAURI_SIGNING_PRIVATE_KEY_PATH);
  const updaterArtifacts = keyInline || keyPath;
  let baseUrl = null;
  if (updaterArtifacts) {
    // The CLI takes the key (or a path to it) from TAURI_SIGNING_PRIVATE_KEY only.
    if (!keyInline) childEnv.TAURI_SIGNING_PRIVATE_KEY = env.TAURI_SIGNING_PRIVATE_KEY_PATH.trim();
    config.bundle = { createUpdaterArtifacts: true };
    if (set(env.ROADEEP_UPDATE_PUBKEY)) {
      // Lets the CLI check that the private key matches the key the app trusts.
      config.plugins = { updater: { pubkey: env.ROADEEP_UPDATE_PUBKEY.trim() } };
    }
    if (!set(env.ROADEEP_RELEASE_BASE_URL)) {
      errors.push(
        "Updater artifacts were requested (TAURI_SIGNING_PRIVATE_KEY is set) but ROADEEP_RELEASE_BASE_URL is missing.\n" +
          "  Set it to where the release files will live, e.g.\n" +
          "  https://github.com/<owner>/<repo>/releases/download/v{version}",
      );
    } else if (!/^https:\/\/[^/\s]+\/\S*$/.test(env.ROADEEP_RELEASE_BASE_URL.trim())) {
      errors.push("ROADEEP_RELEASE_BASE_URL must be an https URL.");
    } else {
      baseUrl = env.ROADEEP_RELEASE_BASE_URL.trim().replace(/\/+$/, "");
    }
    if (!set(env.ROADEEP_UPDATE_URL) || !set(env.ROADEEP_UPDATE_PUBKEY)) {
      warnings.push(
        "ROADEEP_UPDATE_URL / ROADEEP_UPDATE_PUBKEY are not both set: this build publishes update files\n" +
          "  but will not look for updates itself.",
      );
    }
  }

  // ── Authenticode code signing ──
  const thumbprint = set(env.ROADEEP_SIGN_THUMBPRINT) ? env.ROADEEP_SIGN_THUMBPRINT.replace(/\s+/g, "") : null;
  const command = set(env.ROADEEP_SIGN_COMMAND) ? env.ROADEEP_SIGN_COMMAND.trim() : null;
  let signing = null;
  if (thumbprint && command) {
    errors.push("Set ROADEEP_SIGN_THUMBPRINT or ROADEEP_SIGN_COMMAND, not both.");
  } else if (thumbprint) {
    if (!/^[0-9a-fA-F]{40}$/.test(thumbprint)) {
      errors.push("ROADEEP_SIGN_THUMBPRINT must be the certificate's SHA-1 thumbprint (40 hex characters).");
    } else {
      const timestampUrl = set(env.ROADEEP_SIGN_TIMESTAMP_URL) ? env.ROADEEP_SIGN_TIMESTAMP_URL.trim() : DEFAULT_TIMESTAMP_URL;
      signing = { kind: "thumbprint", thumbprint: thumbprint.toUpperCase(), timestampUrl };
      config.bundle = {
        ...config.bundle,
        windows: { certificateThumbprint: signing.thumbprint, digestAlgorithm: "sha256", timestampUrl, tsp: true },
      };
    }
  } else if (command) {
    if (!command.includes("%1")) {
      errors.push("ROADEEP_SIGN_COMMAND must contain %1 where the file to sign goes.");
    } else {
      signing = { kind: "command", command };
      config.bundle = { ...config.bundle, windows: { signCommand: command } };
    }
  }

  return {
    updaterArtifacts,
    baseUrl,
    signing,
    config: Object.keys(config).length > 0 ? config : null,
    childEnv,
    errors,
    warnings,
  };
}

/** Where a release file will be downloaded from; `{version}` in the base is filled in. */
export function releaseAssetUrl(baseUrl, version, fileName) {
  return `${baseUrl.replaceAll("{version}", version).replace(/\/+$/, "")}/${encodeURIComponent(fileName)}`;
}

/** RFC 3339 without milliseconds, as the Tauri updater examples write it. */
export function pubDate(date) {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** The static update manifest tauri-plugin-updater reads (latest.json). */
export function buildLatestJson({ version, notes, date, signature, url }) {
  if (!/^\d+\.\d+\.\d+/.test(version ?? "")) throw new Error(`invalid version: ${version}`);
  if (!set(signature)) throw new Error("the updater signature is empty");
  if (!/^https:\/\//.test(url ?? "")) throw new Error("the installer URL must be https");
  const entry = { signature: signature.trim(), url };
  return {
    version,
    notes: notes ?? "",
    pub_date: pubDate(date),
    platforms: {
      // The plugin looks for "<os>-<arch>-<installer>" first, then "<os>-<arch>".
      "windows-x86_64-nsis": entry,
      "windows-x86_64": entry,
    },
  };
}

/**
 * The CHANGELOG section for this version ("## 0.2.0", "## v0.2.0",
 * "## [0.2.0] - 2026-10-02"…), without its heading; null when there is none.
 */
export function notesFromChangelog(text, version) {
  const lines = (text ?? "").split(/\r?\n/);
  const escaped = version.replace(/\./g, "\\.");
  const heading = new RegExp(`^##\\s+\\[?v?${escaped}\\]?(\\s|$)`);
  const start = lines.findIndex((l) => heading.test(l));
  if (start < 0) return null;
  let end = lines.findIndex((l, i) => i > start && /^##\s/.test(l));
  if (end < 0) end = lines.length;
  const body = lines.slice(start + 1, end).join("\n").trim();
  return body === "" ? null : body;
}

/** owner/repo and tag from a GitHub ".../releases/download/<tag>" base URL. */
export function parseGithubBase(baseUrl, version) {
  const m = /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/releases\/download\/([^/]+)\/?$/.exec(
    baseUrl.replaceAll("{version}", version),
  );
  return m ? { repo: `${m[1]}/${m[2]}`, tag: decodeURIComponent(m[3]) } : null;
}

const quote = (s) => (/^[\w./:@-]+$/.test(s) ? s : `"${s.replace(/(["\\$`])/g, "\\$1")}"`);

/** The command to publish the files (printed by pack, never run by it). */
export function ghReleaseCommand({ repo, tag, title, notesFile, files }) {
  const parts = ["gh", "release", "create", quote(tag), ...files.map(quote)];
  if (repo) parts.push("--repo", quote(repo));
  parts.push("--title", quote(title), "--notes-file", quote(notesFile));
  return parts.join(" ");
}
