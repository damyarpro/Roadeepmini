// The live voice acts in place: a planner change it proposes is shown on the
// planner page itself — a typed-in draft at the top for a new item, or the
// existing row highlighted for complete/delete/log — with the approval controls
// beside it. The decision belongs to src/voice/live.ts; this only renders it.

import { h } from "../dom";
import { formatNumber, t } from "../../core/i18n";
import type { IslandViewName } from "../../core/layout";
import type { ViewHost } from "../views";
import type { LiveApproval, LiveDecision, LivePreview, LiveSnapshot } from "../../voice/live";
import { heardSince, secondsLeft } from "../../local/live-ui";
import { Planner } from "./store";
import "../../local/messages";

type VoicePreview = LivePreview;
/** The preview on an approval, when the runtime made one (planner mutations only). */
export function previewOf(approval: LiveApproval | undefined): LivePreview | undefined {
  const preview = approval?.preview;
  return preview && Array.isArray(preview.fields) && typeof preview.view === "string" ? preview : undefined;
}
/** update/delete highlight the existing row; create is typed in as a new draft entry. */
const existing = (preview: LivePreview | undefined) => !!preview && preview.action !== "create";
export const TYPE_MS = 25;
const SUCCESS_MS = 1400;
const FAILED_MS = 2600;
const FADE_MS = 320;
/** An approved change that never reports back (session closed mid-run) is not left on screen. */
const WORKING_MAX_MS = 20_000;

const reducedMotion = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
const norm = (text: string) => text.normalize("NFC").toLowerCase().replace(/[يى]/g, "ی").replace(/ك/g, "ک").replace(/\s+/g, " ").trim();

/** How much of the fields is typed after `elapsed` ms: one character every TYPE_MS, field after field. */
export function typedFields(fields: VoicePreview["fields"], elapsed: number, instant: boolean, typed = fields.length): { label: string; value: string; done: boolean; context?: boolean }[] {
  let budget = instant ? Infinity : Math.max(0, Math.floor(elapsed / TYPE_MS));
  return fields.map((field, index) => {
    // Fields after `typed` (an update's «متن قبلی») are context, shown whole at once.
    if (index >= typed) return { label: field.label, value: field.value, done: true, context: true };
    const chars = [...field.value];
    const shown = Math.min(chars.length, budget);
    budget -= shown;
    return { label: field.label, value: chars.slice(0, shown).join(""), done: shown === chars.length };
  });
}

/** What the draft shows after its card cleared, from the controller's own decision (never guessed). */
export type DraftEnd = "working" | "done" | "failed" | "gone";
export function draftEnd(decision: LiveDecision | undefined, id: string): DraftEnd {
  if (!decision || decision.id !== id || decision.outcome !== "approved") return "gone";
  return decision.ok === true ? "done" : decision.ok === false ? "failed" : "working";
}

export interface VoiceDraftDeps {
  snapshot(): LiveSnapshot;
  decide(approve: boolean, id: string): void;
  now?(): number;
}

/** Wraps a planner page so it shows the live voice's pending change for `view`. */
export function withVoiceDraft(view: IslandViewName, host: ViewHost, deps: VoiceDraftDeps): ViewHost {
  const now = deps.now ?? (() => Date.now());
  const badge = h("span", { class: "pl-vd-badge" }, h("span", { class: "pl-vd-dot", "aria-hidden": "true" }), h("span", { text: t("liveVoice.draft.waiting") }));
  const countdown = h("span", { class: "pl-vd-countdown", role: "timer", "aria-live": "off" });
  const headline = h("div", { class: "pl-vd-headline", dir: "auto" });
  const fields = h("dl", { class: "pl-vd-fields" });
  const heard = h("div", { class: "pl-vd-heard", dir: "auto", "aria-live": "polite", hidden: true });
  const hint = h("div", { class: "pl-vd-hint", text: t("liveVoice.approvalHint") });
  const reject = h("button", { type: "button", class: "btn secondary" }, h("span", { text: t("liveVoice.reject") }));
  const approve = h("button", { type: "button", class: "btn primary" }, h("span", { text: t("liveVoice.approve") }));
  const done = h("div", { class: "pl-vd-done", role: "status", hidden: true });
  const el = h("section", { class: "pl-voice-draft", hidden: true, "aria-label": t("liveVoice.approvalTitle") },
    h("div", { class: "pl-vd-top" }, badge, headline, countdown), fields,
    h("div", { class: "pl-vd-foot" }, h("div", { class: "pl-vd-notes" }, heard, hint), h("div", { class: "pl-vd-actions" }, reject, approve)), done);

  let current: LiveApproval | undefined;
  let preview: VoicePreview | undefined;
  let baseline = new Map<string, string>();
  let started = 0;
  let typingTimer: ReturnType<typeof setInterval> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  /** The card that cleared and is still being shown (running, done or failed). */
  let ending: { id: string; state: DraftEnd; revision: number } | undefined;

  const answer = (yes: boolean) => { if (current) deps.decide(yes, current.id); };
  approve.onclick = () => answer(true);
  reject.onclick = () => answer(false);

  const body = () => host.el.querySelector<HTMLElement>(".pl-body");
  function mount() { const b = body(); if (b && el.parentElement !== b) b.prepend(el); }
  function stopTyping() { clearInterval(typingTimer); typingTimer = undefined; }
  function clearTarget() { for (const row of host.el.querySelectorAll(".pl-voice-target")) row.classList.remove("pl-voice-target"); }
  function markTarget() {
    clearTarget();
    if (!preview || !current || !existing(preview)) return;
    // Rows carry their item id in data-fk ("check:<id>", "del:<id>", …); titles are the fallback.
    const id = preview.targetId;
    const byId = id ? [...host.el.querySelectorAll<HTMLElement>("[data-fk]")].find((node) => node.dataset.fk?.split(":").slice(1).join(":") === id) : undefined;
    const idRow = byId?.closest<HTMLElement>(".pl-row, li");
    if (idRow) { idRow.classList.add("pl-voice-target"); idRow.scrollIntoView?.({ block: "nearest" }); return; }
    const wanted = preview.fields.map((f) => norm(f.value)).filter(Boolean);
    const titles = [...host.el.querySelectorAll<HTMLElement>(".pl-row .pl-row-title, .pl-note-card .pl-note-text")];
    const hit = titles.find((title) => wanted.some((w) => norm(title.textContent ?? "") === w))
      ?? titles.find((title) => wanted.some((w) => norm(title.textContent ?? "").includes(w)));
    const row = hit?.closest<HTMLElement>(".pl-row, li");
    if (row) { row.classList.add("pl-voice-target"); row.scrollIntoView?.({ block: "nearest" }); }
  }
  function renderFields() {
    if (!preview || !current) return;
    // Only a delete has nothing new to type; an update types its new text (e.g. a note's new wording).
    const typed = typedFields(preview.fields, now() - started, preview.action === "delete" || reducedMotion(), preview.action === "update" ? 1 : preview.fields.length);
    fields.replaceChildren(...typed.flatMap((f, i) => [
      h("dt", { class: f.context ? "is-context" : "", text: f.label }),
      h("dd", { dir: "auto", class: f.context ? "is-context" : i === typed.findIndex((x) => !x.done) ? "typing" : "", title: f.context ? f.value : "", text: f.value }),
    ]));
    if (typed.every((f) => f.done)) stopTyping();
  }
  function show(state: DraftEnd) {
    stopTyping();
    clearTarget();
    el.querySelector<HTMLElement>(".pl-vd-actions")!.hidden = true;
    hint.hidden = true;
    heard.hidden = true;
    el.classList.toggle("is-approved", state === "done");
    el.classList.toggle("is-failed", state === "failed");
    el.classList.toggle("is-working", state === "working");
    el.classList.toggle("is-leaving", state === "gone");
    done.hidden = state === "gone";
    done.textContent = state === "gone" ? "" : t(`liveVoice.draft.${state}`);
    clearTimeout(closeTimer);
    const quick = reducedMotion();
    // done: stays until the real item arrives (planner-changed), then a short flash; failed: an error moment; gone: fade.
    closeTimer = setTimeout(finish, state === "gone" ? (quick ? 0 : FADE_MS) : state === "failed" ? FAILED_MS : state === "done" ? SUCCESS_MS * 3 : WORKING_MAX_MS);
  }
  function finish() {
    clearTimeout(closeTimer);
    ending = undefined;
    el.hidden = true;
    el.classList.remove("is-approved", "is-failed", "is-working", "is-leaving");
    done.hidden = true;
  }
  function open(approval: LiveApproval, next: VoicePreview, snapshot: LiveSnapshot) {
    finish();
    heard.hidden = true;
    current = approval; preview = next; started = now();
    baseline = new Map(snapshot.transcripts.map((line) => [line.id, line.text]));
    el.hidden = false;
    el.querySelector<HTMLElement>(".pl-vd-actions")!.hidden = false;
    hint.hidden = false;
    el.classList.toggle("is-existing", existing(next));
    headline.textContent = existing(next) ? approval.summary : t(`liveVoice.draft.new.${next.kind}`);
    stopTyping();
    if (next.action !== "delete" && !reducedMotion()) typingTimer = setInterval(renderFields, TYPE_MS);
  }

  function syncDraft() {
    mount();
    const snapshot = deps.snapshot();
    const approval = snapshot.approval;
    const next = previewOf(approval);
    const mine = approval && next && next.view === view ? approval : undefined;
    if (mine && mine.id !== current?.id) open(mine, next!, snapshot);
    if (!mine && current) {
      ending = { id: current.id, state: "gone", revision: Planner.revision };
      current = undefined;
      ending.state = draftEnd(snapshot.lastDecision, ending.id);
      show(ending.state);
    } else if (ending && ending.state === "working") {
      const state = draftEnd(snapshot.lastDecision, ending.id);
      if (state !== "working") { ending.state = state; show(state); }
    }
    if (ending?.state === "done" && Planner.revision !== ending.revision) {
      // The real item is on the list now: a short flash, then the draft gets out of the way.
      ending.revision = Planner.revision;
      clearTimeout(closeTimer);
      closeTimer = setTimeout(finish, reducedMotion() ? 0 : SUCCESS_MS);
    }
    if (!current) return;
    renderFields();
    markTarget();
    countdown.textContent = t("liveVoice.approvalLeft", { value: formatNumber(secondsLeft(current.expiresAt, now())) });
    const said = heardSince(snapshot, baseline);
    heard.hidden = !said;
    heard.textContent = said ? t("liveVoice.heard", { text: said }) : "";
  }

  return {
    ...host,
    sync() {
      host.sync();
      syncDraft();
    },
  };
}
