import { CODING_PROVIDERS } from "../activity/providers";
import "./activity.css";
import "../core/locales/activity";
import { h } from "./dom";
import type { ViewHost } from "./views";
import { State } from "../core/state";
import { t, formatNumber } from "../core/i18n";
import { Activity } from "../activity/store";
import { recapMarkdown } from "../activity/recap";
import { Monitor, clearActivity, refreshMonitor, toggleMonitor, toggleHistory, retryHistory } from "../activity/monitor";
import { Bridge, IS_TAURI } from "../core/bridge";
import { createCodingActions, observedUsage } from "./activity-power";
import { activitySessionKey, activitySessionState } from "../activity/island-adapter";
import type { ActivitySession } from "../activity/types";
let activityRevision = 0;
let selectionRevision = 0;
let requestedSelection = "";
/** Only explicit island navigation requests a different evidence session. */
export function requestActivitySession(session: Pick<ActivitySession,"harness" | "id">) {
  requestedSelection = activitySessionKey(session);
  selectionRevision++;
}
Activity.subscribe(() => { activityRevision++; });
interface ActivityUiSnapshot { selected:string; scrollTop:number; diffScroll:number; recapOpen:boolean; focusTarget:string|null }
let captureView: (() => ActivityUiSnapshot) | undefined;
let pendingRestore: ActivityUiSnapshot | undefined;
/** The language switch rebuilds DOM; evidence itself stays in the store. */
export function preserveActivityView() {
  if (State.view === "activity") pendingRestore = captureView?.();
}

export function buildActivity(): ViewHost {
  const sessionKey = activitySessionKey;
  const shortId = (id: string) => id.replace(/^(?:codex|claude):/, "").slice(0,8);
  let initialRestore = pendingRestore;
  pendingRestore = undefined;
  let selected = initialRestore?.selected ?? requestedSelection;
  let appliedSelectionRevision = selectionRevision;
  let renderKey = "";
  let inputKey = "";
  let chooserKey = "";
  let renderedSession = initialRestore?.selected ?? "";
  let exportBusy = false;
  const projectTools = createCodingActions();
  const retentionToggle = h("button", { type: "button", class: "act-clear", onclick: () => void toggleHistory() });
  const retention = h("details", { class: "act-history" },
    h("summary", { text: t("activity.history") }),
    h("p", { class: "act-disclaimer", text: t("activity.historyHint") }), retentionToggle);
  const status = h("span", { class: "act-feedback", role: "status", "aria-live": "polite" });
  const toggle = h("button", { class: "act-toggle", type: "button", onclick: () => void toggleMonitor() });
  const selector = h("select", { "aria-label": t("activity.session"), class: "act-select", onchange: () => {
    selected = selector.value; renderKey = ""; sync(); State.notify();
  } });
  const body = h("div", { class: "act-scroll", tabindex: "0", "aria-label": t("activity.title") });
  const copy = h("button", { type: "button", class: "btn primary", text: t("activity.copy"), onclick: async () => {
    const session = Activity.sessions().find(s => sessionKey(s) === selected);
    if (!session) return;
    status.title = "";
    try { await navigator.clipboard.writeText(recapMarkdown(session, State.settings.language)); status.textContent = t("activity.copied"); }
    catch { status.textContent = t("activity.copyFailed"); void Bridge.log("coding activity handoff copy failed"); }
  } });
  const download = h("button", { type: "button", class: "btn secondary", text: t("activity.export"), onclick: async () => {
    if (exportBusy) return;
    const session = Activity.sessions().find(s => sessionKey(s) === selected);
    if (!session) return;
    exportBusy = true; download.disabled = true; status.textContent=t("activity.exporting"); State.notify(); status.title = "";
    try {
      const content = recapMarkdown(session, State.settings.language);
      if (IS_TAURI) {
        const path = await Bridge.codingExport(content);
        status.textContent = t("activity.exportedNative",{path});
        status.title = path;
        return;
      }
      const url = URL.createObjectURL(new Blob([content], { type: "text/markdown;charset=utf-8" }));
      const a = h("a", { href: url, download: `roadeep-handoff-${session.harness}.md` });
      a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      status.textContent = t("activity.exported");
    } catch { status.textContent = t("activity.exportFailed"); void Bridge.log("coding activity handoff export failed"); }
    finally { exportBusy = false; download.disabled = !Activity.sessions().some(s => sessionKey(s) === selected); State.notify(); }
  } });
  const wipe = h("button", { type: "button", class: "act-clear", text: t("activity.clear"), onclick: () => void clearActivity() });
  const footer = h("div", { class: "act-footer" }, copy, download, wipe);
  const el = h("div", { class: "view activity-view" },
    h("div", { class: "act-heading" }, h("div", { class:"act-title-row" }, h("h2", { text: t("activity.title") }), h("span", { class: "act-eyebrow", text: t("activity.local") })), toggle),
    h("p", { class: "act-privacy", text: t("activity.privacy") }), retention, selector, body, footer, status,
  );
  const focusedPart = () => {
    const focused = document.activeElement as HTMLElement | null;
    const action = focused?.dataset.actFocus;
    if (action) return `.act-project-tools [data-act-focus="${action}"]`;
    if (focused?.matches(".act-history summary")) return ".act-history summary";
    if (focused === retentionToggle) return ".act-history button";
    return focused?.matches(".act-recap summary") ? ".act-recap summary" : focused?.matches(".act-diff pre") ? ".act-diff pre" : focused?.matches(".act-alert button") ? ".act-alert button" : focused === selector ? ".act-select" : focused === body ? ".act-scroll" : null;
  };
  captureView = () => ({selected,scrollTop:body.scrollTop,diffScroll:body.querySelector(".act-diff pre")?.scrollLeft ?? 0,recapOpen:!!body.querySelector<HTMLDetailsElement>(".act-recap")?.open,focusTarget:el.contains(document.activeElement) ? focusedPart() : null});
  function sync() {
    const inputs = JSON.stringify([activityRevision,selectionRevision,selected,State.settings.observeCodex,State.settings.retainCodingHistory,
      Monitor.busy,Monitor.error,Monitor.available,Monitor.operation,Monitor.history.busy,Monitor.history.error,Monitor.history.loaded,State.settings.language,exportBusy]);
    if (inputs === inputKey) return;
    inputKey = inputs;
    if (appliedSelectionRevision !== selectionRevision) {
      selected = requestedSelection;
      appliedSelectionRevision = selectionRevision;
    }
    const sessions = Activity.sessions();
    if (!sessions.some(s => sessionKey(s) === selected)) selected = sessions[0] ? sessionKey(sessions[0]) : "";
    toggle.textContent = t(State.settings.observeCodex ? "activity.disable" : "activity.enable");
    if (Monitor.operation) {
      status.textContent=t(`activity.${Monitor.operation}`); status.dataset.operation="true";
    } else if (status.dataset.operation) { status.textContent=""; delete status.dataset.operation; }
    toggle.disabled = Monitor.busy;
    retentionToggle.textContent = t(State.settings.retainCodingHistory ? "activity.historyDisable" : "activity.historyEnable");
    retentionToggle.setAttribute("aria-pressed", String(State.settings.retainCodingHistory));
    retentionToggle.disabled = Monitor.busy || Monitor.history.busy;
    toggle.setAttribute("aria-pressed", String(State.settings.observeCodex));
    const sessionLabel = (s: typeof sessions[number]) => `${CODING_PROVIDERS[s.harness]} · ${s.cwd?.split(/[\\/]/).filter(Boolean).at(-1) || s.title} · ${shortId(s.id)}`;
    const labels = sessions.map(s => [sessionKey(s),sessionLabel(s)]);
    if (JSON.stringify(labels) !== chooserKey) {
      chooserKey = JSON.stringify(labels);
      selector.replaceChildren(...labels.map(([value,text]) => h("option", { value, text })));
    }
    selector.value = selected;
    selector.hidden = sessions.length === 0;
    copy.disabled = sessions.length === 0;
    download.disabled = sessions.length === 0 || exportBusy;
    wipe.disabled = (sessions.length === 0 && !Monitor.history.error && !State.settings.retainCodingHistory) || Monitor.busy;
    const s = sessions.find(s => sessionKey(s) === selected);
    projectTools.sync(s);
    State.activityBotState = activitySessionState(s);
    const key = JSON.stringify([selected, State.settings.observeCodex, Monitor.error,Monitor.available,Monitor.history.error,State.settings.language,s]);
    if (key === renderKey) return;
    renderKey = key;
    const sameSession = renderedSession === selected;
    const scrollTop = sameSession ? initialRestore?.scrollTop ?? body.scrollTop : 0;
    const diffScroll = sameSession ? initialRestore?.diffScroll ?? body.querySelector(".act-diff pre")?.scrollLeft ?? 0 : 0;
    const recapOpen = sameSession && (initialRestore?.recapOpen ?? !!body.querySelector<HTMLDetailsElement>(".act-recap")?.open);
    const focusTarget = sameSession ? initialRestore?.focusTarget ?? (el.contains(document.activeElement) ? focusedPart() : null) : null;
    const focusedProjectControl = sameSession && projectTools.el.contains(document.activeElement) ? document.activeElement as HTMLElement : null;
    initialRestore = undefined;
    function restoreEvidence() {
      const recap = body.querySelector<HTMLDetailsElement>(".act-recap");
      if (recap) recap.open = recapOpen;
      const diffPre = body.querySelector(".act-diff pre");
      if (diffPre) diffPre.scrollLeft = diffScroll;
      if (focusedProjectControl?.isConnected) focusedProjectControl.focus({preventScroll:true});
      else if (focusTarget) el.querySelector<HTMLElement>(focusTarget)?.focus({preventScroll:true});
      body.scrollTop = scrollTop;
    }
    renderedSession = selected;
    body.replaceChildren();
    if (Monitor.history.error) body.append(h("div", { class: "act-alert", role: "status" }, h("p", { text: t("activity.historyError") }),
      h("button", { type: "button", text: t("activity.retry"), onclick: () => {
        if (State.settings.retainCodingHistory) void retryHistory();
        else void toggleHistory();
      } })));
    if (Monitor.error || (State.settings.observeCodex && !Monitor.available)) {
      body.append(h("div", { class: "act-alert", role: "status" }, h("p", { text: t(Monitor.error ? "activity.error" : "activity.unavailable") }), h("button", { type: "button", text: t("activity.retry"), onclick: () => void refreshMonitor() })));
    }
    if (!s) {
      body.append(h("div", { class: "act-empty" }, h("div", { class: "act-empty-symbol", "aria-hidden": "true", text: "{ }" }), h("h3", { text: t(State.settings.observeCodex ? "activity.empty" : "activity.off") }), h("p", { text: t(State.settings.observeCodex ? "activity.emptySub" : "activity.offSub") })));
      restoreEvidence();
      return;
    }
    body.append(h("div", { class: "act-summary" },
      h("span", { class: `act-status ${s.status}`, text: t(s.status === "error" ? "activity.statusError" : `activity.${s.status}`) }),
      h("span", { text: t(s.changedFiles.length === 1 ? "activity.file" : "activity.files", { count: s.changedFiles.length }) }), h("span", { text: t(s.counts.tools === 1 ? "activity.tool" : "activity.tools", { count: s.counts.tools }) }),
      h("span", { text: t(s.counts.errors === 1 ? "activity.errorCount" : "activity.errors", { count: s.counts.errors }) })),
      h("p", { class: "act-path", dir: "ltr", text: s.cwd ?? s.title }));
    const usage = observedUsage(s);
    if (usage) body.append(usage);
    body.append(projectTools.el);
    if (s.conflicts.length) body.append(h("div", { class: "act-warning" }, h("p", { text: t("activity.conflict", { count: new Set(s.conflicts.map(c => c.file)).size }) }), ...s.conflicts.slice(0, 6).map(c => h("p", { class: "act-conflict-file", dir:"ltr", text: `${c.file} · ${sessions.find(other => other.id === c.sessionId) ? sessionLabel(sessions.find(other => other.id === c.sessionId)!) : shortId(c.sessionId)}` })), h("small", { text: t("activity.conflictSub") })));
    const test = h("section", { class: "act-test" }, h("h3", { text: t("activity.test") }));
    if (s.test) {
      const count = s.test.verdict === "passed" ? s.test.passed : s.test.verdict === "failed" ? s.test.failed : s.test.verdict === "skipped" ? s.test.skipped : 0;
      test.append(h("div", { class: "act-test-result" }, h("strong", { class: `act-verdict ${s.test.verdict}`, text: count ? t(`activity.${s.test.verdict}Count`,{count}) : t(`activity.${s.test.verdict}`) }), h("span", { text: t(s.test.freshness === "unknown" ? "activity.freshUnknown" : `activity.${s.test.freshness}`) })), h("code", { dir: "ltr", text: s.test.command }));
    } else test.append(h("p", { text: t("activity.noTest") }));
    body.append(test);
    const diff = h("section", { class: "act-diff" }, h("div", { class: "act-section-heading" }, h("h3", { text: t("activity.diff") }), s.lastPatch ? h("span", { dir: "ltr", text: `+${formatNumber(s.lastPatch.preview.added)} −${formatNumber(s.lastPatch.preview.removed)}` }) : null));
    if (s.lastPatch) {
      const pre = h("pre", { dir: "ltr", tabindex: "0", "aria-label": t("activity.diff") });
      for (const line of s.lastPatch.preview.lines.filter(line => !/^\*\*\* (?:Begin|End) Patch$/.test(line.text))) pre.append(h("div", { class: `act-line ${line.kind}`, text: line.text }));
      diff.append(pre);
      if (s.lastPatch.preview.truncated) diff.append(h("small", { text: t("activity.truncated") }));
    } else diff.append(h("p", { text: t("activity.noDiff") }));
    body.append(diff);
    if (s.changedFiles.length) body.append(h("section", { class: "act-files" }, h("h3", { text:t("activity.changedFiles") }), ...s.changedFiles.map(file => h("div", { class:"act-file", dir:"ltr", text:file }))));
    const latestError = [...s.events].reverse().find(e => e.kind === "error" || e.phase === "failed");
    if (latestError) body.append(h("div", { class:"act-warning" }, h("h3", { text:t("activity.latestError") }), h("p", { dir:"auto", text:latestError.output?.slice(0, 240) || latestError.title || latestError.tool || t("activity.statusError") })));
    const timeline = h("section", { class:"act-timeline" }, h("h3", { text:t("activity.recent") }));
    for (const event of s.events.filter(e => e.kind === "tool" || e.kind === "finished" || e.kind === "error" || e.kind === "cancelled").slice(-5).reverse()) timeline.append(h("div", { class:"act-timeline-row" }, h("span", { dir:"ltr", text:new Date(event.at).toLocaleTimeString(State.settings.language === "fa" ? "fa-IR" : "en-US", {hour:"2-digit",minute:"2-digit"}) }), h("span", { dir:"auto", text:event.command?.slice(0, 100) || event.title || event.tool || t(event.kind === "finished" ? "activity.finished" : event.kind === "cancelled" ? "activity.cancelled" : "activity.statusError") }), h("span", { class:"act-timeline-phase", text: event.kind === "finished" ? t("activity.finished") : event.kind === "cancelled" ? t("activity.cancelled") : event.kind === "error" || event.phase === "failed" ? t("activity.failed") : event.phase === "completed" ? t("activity.observed") : event.phase === "started" ? t("activity.active") : t("activity.unknown") })));
    body.append(timeline, h("details", { class: "act-recap" }, h("summary", { text: t("activity.recap") }), h("pre", { dir:"ltr", text: recapMarkdown(s, State.settings.language) })), h("p", { class: "act-disclaimer", text: t("activity.disclaimer") }));
    restoreEvidence();
  }
  return { el, sync };
}
