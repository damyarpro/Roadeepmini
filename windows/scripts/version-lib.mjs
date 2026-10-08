// Pure helpers for scripts/version.mjs. The version lives in four places that
// must agree (the Windows workflow checks the first three against the tag):
// package.json (+ package-lock.json), src-tauri/tauri.conf.json, and the
// [workspace.package] version in Cargo.toml (+ the workspace crates in Cargo.lock).

/** X.Y.Z, plain numbers: what NSIS, the updater and the workflow all accept. */
export function validVersion(v) {
  return typeof v === "string" && /^(0|[1-9]\d{0,4})\.(0|[1-9]\d{0,4})\.(0|[1-9]\d{0,4})$/.test(v);
}

/** Replaces the first `"version": "…"` — the top-level one in package.json and tauri.conf.json. */
export function bumpJsonVersion(text, version) {
  const re = /^(\s*"version"\s*:\s*")[^"]*(")/m;
  if (!re.test(text)) throw new Error('no "version" field');
  return text.replace(re, `$1${version}$2`);
}

/** package-lock.json: the root version and the root package entry, nothing else. */
export function bumpPackageLock(text, version) {
  const lock = JSON.parse(text);
  lock.version = version;
  if (lock.packages && lock.packages[""]) lock.packages[""].version = version;
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const out = JSON.stringify(lock, null, 2).replace(/\n/g, eol);
  return text.endsWith("\n") ? out + eol : out;
}

/** The `version` line of the [workspace.package] table. */
export function bumpCargoToml(text, version) {
  const lines = text.split(/(?<=\n)/);
  let inTable = false;
  let done = false;
  const out = lines.map((line) => {
    const header = /^\s*\[([^\]]+)\]\s*$/.exec(line.trimEnd());
    if (header) inTable = header[1].trim() === "workspace.package";
    if (inTable && !done && /^\s*version\s*=/.test(line)) {
      done = true;
      return line.replace(/^(\s*version\s*=\s*")[^"]*(")/, `$1${version}$2`);
    }
    return line;
  });
  if (!done) throw new Error("no version in [workspace.package]");
  return out.join("");
}

/** The `version` of the named [[package]] entries (the workspace crates). */
export function bumpCargoLock(text, names, version) {
  const wanted = new Set(names);
  const found = new Set();
  const out = text.replace(
    /(\[\[package\]\]\r?\nname = "([^"]+)"\r?\nversion = ")[^"]*(")/g,
    (all, head, name, tail) => {
      if (!wanted.has(name)) return all;
      found.add(name);
      return `${head}${version}${tail}`;
    },
  );
  const missing = names.filter((n) => !found.has(n));
  if (missing.length > 0) throw new Error(`Cargo.lock has no entry for ${missing.join(", ")}`);
  return out;
}

/** The version each file currently declares, to report a drift before bumping. */
export function currentVersions({ packageJson, tauriConf, cargoToml }) {
  const json = (t) => /^\s*"version"\s*:\s*"([^"]*)"/m.exec(t)?.[1] ?? null;
  let cargo = null;
  let inTable = false;
  for (const line of cargoToml.split(/\r?\n/)) {
    const header = /^\s*\[([^\]]+)\]\s*$/.exec(line);
    if (header) inTable = header[1].trim() === "workspace.package";
    const m = inTable ? /^\s*version\s*=\s*"([^"]*)"/.exec(line) : null;
    if (m) {
      cargo = m[1];
      break;
    }
  }
  return { packageJson: json(packageJson), tauriConf: json(tauriConf), cargoToml: cargo };
}
