// Settings → General → Updates. Talks to src-tauri/src/updater.rs through
// core/bridge-update.ts. Checking is harmless and can happen on its own; the
// download and the install only ever start from the "Install and restart" click.
// Release notes come from the network: they are shown with textContent only.

import { Bridge } from "../core/bridge";
import { BridgeUpdate, type UpdateStatus } from "../core/bridge-update";
import { localizeError } from "../core/error-text";
import { dateLocale, formatNumber, isolate, t } from "../core/i18n";
import type { Settings } from "../core/state";
import { h, clear } from "../views/dom";
import { settingRow, statusBadge, switchEl } from "./ui";

export interface UpdateHost {
  settings: () => Settings;
  save: () => Promise<void>;
  /** The app version from boot, shown while the status is not known yet. */
  version: () => string;
}

/** Last known state, kept across redraws (a language change rebuilds the section). */
let status: UpdateStatus | null = null;
let loaded = false;
let installing = false;
/** What the retry button repeats. */
let lastAction: "check" | "install" = "check";
/** Install errors live here; check errors come back in `status.error`. */
let installError: string | null = null;
let listening = false;
let redraw: (() => void) | null = null;

function formatWhen(ms: number, withTime: boolean): string {
  const opts: Intl.DateTimeFormatOptions = withTime
    ? { dateStyle: "medium", timeStyle: "short" }
    : { dateStyle: "medium" };
  try {
    return new Intl.DateTimeFormat(dateLocale(), opts).format(new Date(ms));
  } catch {
    return new Date(ms).toISOString();
  }
}

function listenOnce() {
  if (listening) return;
  listening = true;
  void BridgeUpdate.onAvailable((available) => {
    if (status) status = { ...status, available, error: null };
    void refresh();
  });
  void BridgeUpdate.onProgress((p) => {
    if (!status) return;
    status = { ...status, progress: p.progress, downloaded: p.progress != null && p.progress >= 1 };
    redraw?.();
  });
}

async function refresh() {
  const next = await BridgeUpdate.status();
  loaded = true;
  if (next) status = next;
  redraw?.();
}

async function check() {
  lastAction = "check";
  installError = null;
  if (status) status = { ...status, checking: true, error: null };
  redraw?.();
  const next = await BridgeUpdate.check();
  if (next) status = next;
  else if (status) status = { ...status, checking: false };
  redraw?.();
}

async function install() {
  if (installing || !status?.available) return;
  lastAction = "install";
  installing = true;
  installError = null;
  status = { ...status, error: null };
  redraw?.();
  try {
    // On success the app quits here and the installer reopens it.
    await BridgeUpdate.install();
  } catch (err) {
    installError = String(err);
  }
  installing = false;
  await refresh();
}

/**
 * The Updates rows for the General card. Returned as separate rows so the card's
 * row dividers apply; rows that have nothing to show are hidden.
 */
export function updateRows(host: UpdateHost): HTMLElement[] {
  listenOnce();

  const stateText = h("span", { class: "set-hint", role: "status", "aria-live": "polite" });
  const badgeSlot = h("span", { class: "update-badge" });
  const checkBtn = h("button", { type: "button", class: "sm", text: t("update.check") }) as HTMLButtonElement;
  checkBtn.addEventListener("click", () => void check());

  const main = settingRow({ label: t("update.title"), extra: stateText }, badgeSlot, checkBtn);
  main.classList.add("update-row");

  const detail = h("div", { class: "set-row stack update-detail" });
  const auto = settingRow(
    { label: t("update.autoCheck"), hint: t("update.autoCheckHint") },
    switchEl(host.settings().autoUpdateCheck !== false, false, t("update.autoCheck"), (v) => {
      host.settings().autoUpdateCheck = v;
      void host.save().catch((err) => void Bridge.log(`settings: saving autoUpdateCheck failed: ${String(err)}`));
    }),
  );

  function draw() {
    const s = status;
    const enabled = s?.enabled === true;
    const version = (s?.currentVersion || host.version()).replace(/^v/i, "");

    // Not built with the updater (or a plain browser): one quiet line, no controls.
    if (loaded && !enabled) {
      main.classList.add("off");
      stateText.textContent = t("update.disabled");
      clear(badgeSlot);
      checkBtn.hidden = true;
      detail.hidden = true;
      auto.hidden = true;
      return;
    }
    main.classList.remove("off");
    checkBtn.hidden = false;
    auto.hidden = !enabled;

    const downloading = !!s && s.progress != null && !s.downloaded;
    const busy = !s || s.checking || downloading || installing;
    checkBtn.disabled = busy;
    checkBtn.textContent = s?.checking ? t("update.checking") : t("update.check");

    const versionText = version ? t("update.version", { version: isolate(version) }) : "";
    const parts = [versionText];
    if (s?.lastCheckedAt && !s.checking) parts.push(t("update.lastChecked", { time: formatWhen(s.lastCheckedAt, true) }));
    stateText.textContent = parts.filter(Boolean).join(" · ");

    clear(badgeSlot);
    if (s && !s.checking && s.lastCheckedAt && !s.available && !s.error) {
      badgeSlot.append(statusBadge("ok", t("update.upToDate")));
    }

    clear(detail);
    const error = installError ?? s?.error ?? null;
    if (!s || (!s.available && !error)) {
      detail.hidden = true;
      return;
    }
    detail.hidden = false;

    if (s.available) {
      const a = s.available;
      const title = h("div", { class: "update-head" },
        h("span", { class: "set-label", text: t("update.available", { version: isolate(a.version) }) }),
        a.date ? h("span", { class: "set-hint", text: formatWhen(a.date, false) }) : null,
      );
      detail.append(title);
      if (a.notes) {
        detail.append(
          h("span", { class: "update-notes-label", text: t("update.notes") }),
          h("div", { class: "update-notes", dir: "auto", text: a.notes }),
        );
      }

      if (downloading || installing) {
        const pct = s.progress ?? 0;
        const label = installing && (s.downloaded || pct >= 1)
          ? t("update.installing")
          : s.progress != null
            ? t("update.downloading", { percent: formatNumber(pct, { style: "percent", maximumFractionDigits: 0 }) })
            : t("update.downloadingNoSize");
        const bar = h("div", {
          class: "update-progress", role: "progressbar",
          "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(Math.round(pct * 100)),
          "aria-label": label,
        }, h("span", { style: `inline-size: ${Math.round(pct * 100)}%` }));
        detail.append(h("div", { class: "update-busy" }, bar, h("span", { class: "set-hint", text: label })));
      } else {
        const installBtn = h("button", { type: "button", class: "primary", text: t("update.install") }) as HTMLButtonElement;
        installBtn.addEventListener("click", () => void install());
        detail.append(h("div", { class: "update-actions" },
          installBtn,
          h("span", { class: "set-hint", text: t("update.installHint") }),
        ));
      }
    }

    if (error && !downloading && !installing) {
      // After a failed install the install button above is the retry.
      const retry = lastAction === "install" && s.available
        ? null
        : h("button", { type: "button", class: "sm", text: t("update.retry"), onclick: () => void check() });
      detail.append(h("div", { class: "update-error" },
        h("span", { class: "field-err", role: "alert", dir: "auto", text: localizeError(error) }),
        retry,
      ));
    }
  }

  redraw = draw;
  draw();
  if (!loaded) void refresh();
  return [main, detail, auto];
}
