// English strings for the Updates row (Settings → General, settings/update-row.ts)
// and the E_UPDATE_* codes from src-tauri/src/updater.rs. Registered through
// registerMessages by core/error-text.ts.

export const updateEn: Record<string, string> = {
  "update.title": "Updates",
  "update.version": "Version {version}",
  "update.check": "Check for updates",
  "update.checking": "Checking…",
  "update.upToDate": "Up to date",
  "update.lastChecked": "Last checked {time}",
  "update.available": "Version {version} is ready",
  "update.notes": "What’s new",
  "update.install": "Install and restart",
  "update.installHint": "Roadeep closes, installs the update and opens again.",
  "update.downloading": "Downloading… {percent}",
  "update.downloadingNoSize": "Downloading…",
  "update.installing": "Starting the installer…",
  "update.retry": "Try again",
  "update.autoCheck": "Check automatically",
  "update.autoCheckHint": "Once a day. Nothing is downloaded or installed without your click.",
  "update.disabled": "Automatic updates are not enabled in this build",

  "err.update.disabled": "Automatic updates are not enabled in this build.",
  "err.update.nothing": "There is no update to install. Check again.",
  "err.update.busy": "An update check or download is already running.",
  "err.update.network": "Could not reach the update server. Check your connection or proxy.",
  "err.update.timeout": "The update server took too long to answer.",
  "err.update.noRelease": "No published release was found.",
  "err.update.manifest": "The release information could not be read.",
  "err.update.signature": "The download failed its signature check and was discarded.",
  "err.update.install": "The installer could not be started.",
  "err.update.failed": "The update failed.",
};
