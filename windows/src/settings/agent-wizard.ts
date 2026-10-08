// The agent builder: a five-step dialog in the settings window.
//
//   1 Goal   — what the agent is for, which model drafts it, an optional base
//   2 Draft  — the model's proposal, every field editable
//   3 Refine — feedback → a revised draft; each revision can be undone
//   4 Look   — name and pill colour, with a live mini character
//   5 Save   — summary, "show next to the character now", save to agents.json
//
// Drafting goes through Rust (agent_draft): a one-off Roadeep thread, never the
// island's conversation. Only lengths and codes are logged, never the text.

import {
  Bridge, isRoadeepError, LOCAL_AGENT_LIMITS,
  type AgentSuggestion, type LocalAgent, type LocalAgentDraft, type RoadeepModel,
} from "../core/bridge";
import {
  AGENT_PALETTE, MAX_ACTIVE_PILLS, agentColor, agentPillId, isHexColor, type AgentTask, type Settings,
} from "../core/state";
import { formatNumber, getLanguage, t } from "../core/i18n";
import { memoryTriggerWords } from "../core/memory-words";
import { h, clear } from "../views/dom";
import { fieldErrors, roadeepErrorText } from "../views/errors";
import { createMiniBot, pruneMiniBots, tickMiniBots } from "../character/minibots";
import { roadeepData } from "./roadeep-section";
import { isPillOn, pillsFull, setPill } from "./pills";
import { charCount, field, fieldError, followTextDirection, icon, switchEl } from "./ui";

export interface WizardHost {
  settings: () => Settings;
  /** After a save: re-read the list, say what happened. */
  saved: (agent: LocalAgent) => void;
}

type Step = 1 | 2 | 3 | 4 | 5;
const STEPS: Step[] = [1, 2, 3, 4, 5];
const STEP_KEYS: Record<Step, string> = {
  1: "wizard.stepGoal", 2: "wizard.stepDraft", 3: "wizard.stepRefine", 4: "wizard.stepLook", 5: "wizard.stepSave",
};
const MAX_GOAL = 4000;
const MAX_FEEDBACK = 2000;
const PROMPT_FIELDS = 3;

interface Wizard {
  host: WizardHost;
  step: Step;
  /** Furthest step the user may jump to from the stepper. */
  reached: Step;
  editingId: string | null;
  goal: string;
  model: string;
  baseAgentId: string | null;
  draft: AgentSuggestion | null;
  /** Earlier drafts, newest last: "Undo" pops one. */
  undo: AgentSuggestion[];
  feedback: string;
  color: string;
  showPill: boolean;
  busy: "draft" | "refine" | "save" | null;
  error: string | null;
  /** THROTTLED: drafting is blocked until this time (ms). */
  retryAt: number;
  errors: Record<string, string>;
  confirmClose: boolean;
  /** Draft fields edited since the last save (closing asks first). */
  dirty: boolean;
}

let wiz: Wizard | null = null;
let backdrop: HTMLElement | null = null;
let dialog: HTMLElement | null = null;
let restoreFocus: HTMLElement | null = null;
let frame = 0;
let ticker = 0;
let entering = false;

function log(message: string) {
  void Bridge.log(`settings: wizard ${message}`);
}

function emptyDraft(): AgentSuggestion {
  return { name: "", description: "", instructions: "", starterPrompts: [], webSearch: false, suggestedColor: null };
}

const signedIn = () => roadeepData().session?.signedIn === true;
const throttleLeft = () => (wiz ? Math.max(0, Math.ceil((wiz.retryAt - Date.now()) / 1000)) : 0);

// ── Open / close ──────────────────────────────────────────────────────────────

/** Opens the builder: empty, or on the Draft step of an existing agent. */
export function openAgentWizard(host: WizardHost, editing?: LocalAgent) {
  if (wiz) return;
  restoreFocus = document.activeElement as HTMLElement | null;
  const s = host.settings();
  wiz = {
    host,
    step: editing ? 2 : 1,
    reached: editing ? 5 : 1,
    editingId: editing?.id ?? null,
    goal: editing?.description ?? "",
    model: editing?.model ?? s.model ?? "",
    baseAgentId: editing?.baseAgentId ?? null,
    draft: editing
      ? {
          name: editing.name, description: editing.description, instructions: editing.instructions,
          starterPrompts: [...editing.starterPrompts], webSearch: editing.webSearch,
          suggestedColor: editing.color || null,
        }
      : null,
    undo: [],
    feedback: "",
    color: editing ? agentColor(editing.id, editing.color) : AGENT_PALETTE[0],
    showPill: editing ? isPillOn(agentPillId("local", editing.id)) : !pillsFull(),
    busy: null,
    error: null,
    retryAt: 0,
    errors: {},
    confirmClose: false,
    dirty: false,
  };
  backdrop = h("div", { class: "wiz-backdrop" });
  backdrop.addEventListener("mousedown", (e) => {
    if (e.target === backdrop) requestClose();
  });
  document.body.append(backdrop);
  document.body.classList.add("wiz-open");
  document.addEventListener("keydown", onKey, true);
  ticker = window.setInterval(onTick, 1000);
  entering = true;
  log(editing ? "opened to edit" : "opened");
  render();
}

function close() {
  if (!wiz) return;
  // Closing mid-draft stops paying for a draft nobody will see.
  if (wiz.busy === "draft" || wiz.busy === "refine") stopDraft();
  wiz = null;
  cancelAnimationFrame(frame);
  frame = 0;
  window.clearInterval(ticker);
  document.removeEventListener("keydown", onKey, true);
  backdrop?.remove();
  backdrop = dialog = null;
  document.body.classList.remove("wiz-open");
  pruneMiniBots();
  restoreFocus?.focus?.();
}

function requestClose() {
  if (!wiz) return;
  if (wiz.dirty || wiz.busy) {
    wiz.confirmClose = true;
    render();
    return;
  }
  close();
}

function onKey(e: KeyboardEvent) {
  if (!wiz || !dialog) return;
  if (e.key === "Escape") {
    e.preventDefault();
    if (wiz.confirmClose) {
      wiz.confirmClose = false;
      render();
    } else {
      requestClose();
    }
    return;
  }
  // Keep Tab inside the dialog.
  if (e.key === "Tab") {
    const items = [...dialog.querySelectorAll<HTMLElement>(
      "button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)",
    )].filter((el) => el.offsetParent !== null);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
}

/** Buttons that ask the model wait out a THROTTLED and need a session. */
function syncAiButtons() {
  if (!wiz) return;
  for (const b of dialog?.querySelectorAll<HTMLButtonElement>("button[data-needs-ai]") ?? []) {
    b.disabled = throttleLeft() > 0 || !!wiz.busy || b.dataset.blocked === "1";
  }
}

/** Keeps the THROTTLED countdown live. */
function onTick() {
  if (!wiz || wiz.retryAt === 0) return;
  if (throttleLeft() === 0) wiz.retryAt = 0;
  const note = dialog?.querySelector<HTMLElement>("[data-throttle]");
  if (note) note.textContent = throttleLeft() > 0 ? t("rerr.THROTTLED.wait", { s: throttleLeft() }) : "";
  syncAiButtons();
}

// ── Drafting ──────────────────────────────────────────────────────────────────

async function runDraft(kind: "draft" | "refine") {
  const w = wiz;
  if (!w || w.busy || throttleLeft() > 0) return;
  const refine = kind === "refine" && w.draft != null;
  w.busy = kind;
  w.error = null;
  render();
  try {
    const result = await Bridge.agentDraft({
      // An agent written by hand has no goal: what it already is stands in.
      goal: w.goal.trim() || w.draft?.description || w.draft?.name || w.draft?.instructions.slice(0, 1000) || "",
      model: w.model,
      language: getLanguage(),
      previous: refine ? w.draft : null,
      feedback: refine ? w.feedback : null,
    });
    if (wiz !== w) return;
    if (w.draft) w.undo.push(w.draft);
    w.draft = result;
    w.dirty = true;
    if (!refine) w.color = isHexColor(result.suggestedColor) ? result.suggestedColor.toUpperCase() : w.color;
    if (refine) w.feedback = "";
    if (w.step === 1) w.step = 2;
    w.reached = 5;
    log(`${kind} ok`);
  } catch (err) {
    if (wiz !== w) return;
    const code = isRoadeepError(err) ? err.code : "UNKNOWN";
    if (code === "CANCELLED") {
      w.error = null;
    } else if (code === "THROTTLED") {
      // The live countdown line says it; no second, frozen copy.
      w.retryAt = Date.now() + (isRoadeepError(err) && err.retryAfter ? err.retryAfter : 60) * 1000;
      w.error = null;
      log(`${kind} throttled`);
    } else {
      w.error = roadeepErrorText(err);
      log(`${kind} failed ${code}`);
    }
  } finally {
    if (wiz === w) {
      w.busy = null;
      render();
    }
  }
}

function stopDraft() {
  log("stop");
  Bridge.agentDraftCancel().catch((err) => log(`stop failed ${isRoadeepError(err) ? err.code : String(err)}`));
}

function undoDraft() {
  const w = wiz;
  if (!w || !w.undo.length || w.busy) return;
  w.draft = w.undo.pop()!;
  w.dirty = true;
  render();
}

// ── Validation and save ───────────────────────────────────────────────────────

function validate(d: AgentSuggestion): Record<string, string> {
  const errors: Record<string, string> = {};
  const name = charCount(d.name.trim());
  if (name === 0 || name > LOCAL_AGENT_LIMITS.name) errors.name = t("agents.nameRequired");
  const instr = charCount(d.instructions.trim());
  if (instr === 0 || instr > LOCAL_AGENT_LIMITS.instructions) errors.instructions = t("agents.instructionsRequired");
  if (charCount(d.description.trim()) > LOCAL_AGENT_LIMITS.description) errors.description = t("wizard.descriptionTooLong");
  if (d.starterPrompts.some((p) => charCount(p.trim()) > LOCAL_AGENT_LIMITS.starterPrompt)) {
    errors.starterPrompts = t("wizard.promptTooLong");
  }
  return errors;
}

/** First step holding a field with an error. */
function stepOf(errors: Record<string, string>): Step {
  if (errors.name && !errors.instructions && !errors.description && !errors.starterPrompts) return 4;
  return 2;
}

async function save() {
  const w = wiz;
  if (!w || w.busy || !w.draft) return;
  w.errors = validate(w.draft);
  if (Object.keys(w.errors).length) {
    w.step = stepOf(w.errors);
    render();
    return;
  }
  const d = w.draft;
  const payload: LocalAgentDraft = {
    id: w.editingId,
    name: d.name.trim(),
    instructions: d.instructions.trim(),
    description: d.description.trim(),
    model: w.model,
    webSearch: d.webSearch,
    baseAgentId: w.baseAgentId,
    color: w.color,
    starterPrompts: d.starterPrompts.map((p) => p.trim()).filter(Boolean).slice(0, LOCAL_AGENT_LIMITS.starterPrompts),
  };
  w.busy = "save";
  w.error = null;
  render();
  try {
    const saved = await Bridge.localAgentSave(payload);
    if (wiz !== w) return;
    const pill = agentPillId("local", saved.id);
    if (w.showPill && !isPillOn(pill)) setPill(pill, true);
    if (!w.showPill && isPillOn(pill)) setPill(pill, false);
    log(w.editingId ? "saved an edit" : "created an agent");
    const host = w.host;
    w.dirty = false;
    close();
    host.saved(saved);
  } catch (err) {
    if (wiz !== w) return;
    log(`save failed ${isRoadeepError(err) ? err.code : String(err)}`);
    const fields = fieldErrors(err);
    w.errors = {};
    if (fields.name) w.errors.name = t("agents.nameRequired");
    if (fields.instructions) w.errors.instructions = t("agents.instructionsRequired");
    if (fields.description) w.errors.description = t("wizard.descriptionTooLong");
    if (fields.starterPrompts) w.errors.starterPrompts = t("wizard.promptTooLong");
    if (fields.color) w.errors.color = fields.color;
    if (Object.keys(w.errors).length) w.step = w.errors.color ? 4 : stepOf(w.errors);
    w.error = roadeepErrorText(err);
    w.busy = null;
    render();
  }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

function modelLabel(m: RoadeepModel): string {
  return m.displayName || m.id;
}

function goTo(step: Step) {
  if (!wiz || wiz.busy) return;
  wiz.step = step;
  wiz.reached = Math.max(wiz.reached, step) as Step;
  wiz.confirmClose = false;
  render();
}

function render() {
  const w = wiz;
  if (!w || !backdrop) return;
  const keepFocusId = (document.activeElement as HTMLElement | null)?.id;
  cancelAnimationFrame(frame);
  frame = 0;
  clear(backdrop);

  const title = t(w.editingId ? "wizard.titleEdit" : "wizard.title");
  const closeBtn = h("button", {
    type: "button", class: "wiz-close", "aria-label": t("wizard.close"), title: t("wizard.close"),
    onclick: requestClose,
  }, icon("close", 18));
  // Only the first render rises in; steps swap in place.
  dialog = h("div", { class: entering ? "wiz enter" : "wiz", role: "dialog", "aria-modal": "true", "aria-labelledby": "wiz-title" },
    h("div", { class: "wiz-head" }, h("h2", { id: "wiz-title", text: title }), closeBtn),
    stepper(w),
  );
  const body = h("div", { class: "wiz-body" });
  switch (w.step) {
    case 1: goalStep(w, body); break;
    case 2: draftStep(w, body); break;
    case 3: refineStep(w, body); break;
    case 4: lookStep(w, body); break;
    case 5: saveStep(w, body); break;
  }
  if (w.error) body.append(h("div", { class: "notice err", role: "alert", dir: "auto", text: w.error }));
  body.append(h("div", { class: "hint throttle", "data-throttle": "1", role: "status",
    text: throttleLeft() > 0 ? t("rerr.THROTTLED.wait", { s: throttleLeft() }) : "" }));
  dialog.append(body, footer(w));
  backdrop.append(dialog);
  entering = false;
  syncAiButtons();

  const again = keepFocusId ? document.getElementById(keepFocusId) : null;
  if (again && dialog.contains(again)) again.focus();
  else dialog.querySelector<HTMLElement>("[data-autofocus]")?.focus();
}

function stepper(w: Wizard): HTMLElement {
  const list = h("ol", { class: "wiz-steps" });
  for (const s of STEPS) {
    const reachable = s <= w.reached && (s === 1 || w.draft != null) && !w.busy;
    const item = h("li", { class: s === w.step ? "on" : s < w.step ? "done" : "" });
    const btn = h("button", {
      type: "button",
      "aria-current": s === w.step ? "step" : undefined,
      onclick: () => goTo(s),
    },
      h("span", { class: "num", text: formatNumber(s) }),
      h("span", { class: "lbl", text: t(STEP_KEYS[s]) }),
    ) as HTMLButtonElement;
    btn.disabled = !reachable || s === w.step;
    item.append(btn);
    list.append(item);
  }
  return list;
}

function counter(text: string, max: number): HTMLElement {
  return h("span", { class: "hint counter", text: t("agents.count", { n: charCount(text), max }) });
}

/** Textarea with a live counter in its label row. */
function textArea(id: string, label: string, value: string, max: number, rows: number, opts: {
  placeholder?: string; error?: string; onInput: (v: string) => void; autofocus?: boolean;
}): HTMLElement {
  const area = h("textarea", { id, rows: String(rows), maxlength: String(max), placeholder: opts.placeholder ?? "" }) as HTMLTextAreaElement;
  area.value = value;
  if (opts.autofocus) area.dataset.autofocus = "1";
  followTextDirection(area);
  const count = counter(value, max);
  area.addEventListener("input", () => {
    opts.onInput(area.value);
    count.textContent = t("agents.count", { n: charCount(area.value), max });
  });
  if (opts.error) area.setAttribute("aria-invalid", "true");
  return h("div", { class: "field" },
    h("div", { class: "label-row" }, h("label", { for: id, text: label }), count),
    area,
    fieldError(opts.error),
  );
}

/**
 * Under the instructions: the phrases Roadeep's server takes for a "save to
 * memory" request (the agent would then get a canned memory reply instead of an
 * answer). A warning only; saving stays allowed.
 */
function memoryWarning(text: string): { el: HTMLElement; update(text: string): void } {
  const words = h("span");
  const el = h("div", { class: "notice warn wiz-memory", role: "status", "aria-live": "polite" },
    h("span", { text: t("agents.memoryWarn") }), " ", words, ". ",
    h("span", { text: t("agents.memoryWarnFix") }),
  );
  const fa = getLanguage() === "fa";
  const sep = fa ? "، " : ", ";
  const quote = (w: string) => (fa ? `«${w}»` : `“${w}”`);
  let shown = "";
  const update = (next: string) => {
    const found = memoryTriggerWords(next);
    const key = found.join("\n");
    if (key === shown) return;
    shown = key;
    el.hidden = !found.length;
    words.replaceChildren(...found.flatMap((w, i) => [
      ...(i ? [sep] : []),
      h("bdi", { dir: "auto", text: quote(w) }),
    ]));
  };
  el.hidden = true;
  update(text);
  return { el, update };
}

const MEMORY_WARN_DEBOUNCE_MS = 250;

function textInput(id: string, value: string, max: number, onInput: (v: string) => void): HTMLInputElement {
  const input = h("input", { type: "text", maxlength: String(max), value }) as HTMLInputElement;
  input.id = id;
  followTextDirection(input);
  input.addEventListener("input", () => onInput(input.value));
  return input;
}

function modelSelect(w: Wizard): HTMLSelectElement {
  const models = roadeepData().models ?? [];
  const select = h("select", {}) as HTMLSelectElement;
  const fallback = models.find((m) => m.isDefault);
  select.append(h("option", {
    value: "",
    text: fallback ? t("account.serverDefaultNamed", { name: modelLabel(fallback) }) : t("account.serverDefault"),
  }));
  for (const m of models) select.append(h("option", { value: m.id, text: modelLabel(m) }));
  if (w.model && !models.some((m) => m.id === w.model)) select.append(h("option", { value: w.model, text: w.model }));
  select.value = w.model;
  select.addEventListener("change", () => {
    w.model = select.value;
    w.dirty = true;
  });
  return select;
}

function baseSelect(w: Wizard): HTMLSelectElement | null {
  const cat = roadeepData().catalog;
  const agents = cat ? [...cat.exclusive, ...cat.public] : [];
  if (!agents.length && !w.baseAgentId) return null;
  const select = h("select", {}) as HTMLSelectElement;
  select.append(h("option", { value: "", text: t("agents.baseNone") }));
  for (const a of agents) select.append(h("option", { value: a.id, text: a.title }));
  if (w.baseAgentId && !agents.some((a) => a.id === w.baseAgentId)) {
    select.append(h("option", { value: w.baseAgentId, text: "…" }));
  }
  select.value = w.baseAgentId ?? "";
  select.addEventListener("change", () => {
    w.baseAgentId = select.value || null;
    w.dirty = true;
  });
  return select;
}

function busyLine(w: Wizard): HTMLElement | null {
  if (w.busy !== "draft" && w.busy !== "refine") return null;
  return h("div", { class: "wiz-busy", role: "status" },
    h("span", { class: "spinner", "aria-hidden": "true" }),
    h("span", { text: t(w.busy === "draft" ? "wizard.drafting" : "wizard.refining") }),
  );
}

function goalStep(w: Wizard, body: HTMLElement) {
  body.append(h("p", { class: "hint", text: t("wizard.goalHint") }));
  body.append(textArea("wiz-goal", t("wizard.goal"), w.goal, MAX_GOAL, 5, {
    placeholder: t("wizard.goalPlaceholder"),
    error: w.errors.goal,
    autofocus: true,
    onInput: (v) => {
      w.goal = v;
      w.dirty = true;
      if (w.errors.goal && v.trim()) {
        delete w.errors.goal;
        const err = dialog?.querySelector<HTMLElement>("#wiz-goal + .field-err");
        if (err) err.textContent = "";
        dialog?.querySelector("#wiz-goal")?.removeAttribute("aria-invalid");
      }
    },
  }));
  const row = h("div", { class: "wiz-row" },
    field("wiz-model", t("wizard.model"), modelSelect(w)),
  );
  const base = baseSelect(w);
  if (base) row.append(field("wiz-base", t("agents.baseAgent"), base));
  body.append(row, h("div", { class: "hint", text: t("wizard.modelHint") }));
  if (!signedIn()) body.append(h("div", { class: "notice warn", text: t("wizard.signedOut") }));
  const busy = busyLine(w);
  if (busy) body.append(busy);
}

function draftStep(w: Wizard, body: HTMLElement) {
  const d = (w.draft ??= emptyDraft());
  const edited = () => { w.dirty = true; };
  body.append(h("p", { class: "hint", text: t(w.undo.length || w.reached > 2 ? "wizard.draftHint" : "wizard.draftHintManual") }));

  const name = textInput("wiz-name", d.name, LOCAL_AGENT_LIMITS.name, (v) => { d.name = v; edited(); });
  name.dataset.autofocus = "1";
  const desc = textInput("wiz-desc", d.description, LOCAL_AGENT_LIMITS.description, (v) => { d.description = v; edited(); });
  const memory = memoryWarning(d.instructions);
  let memoryTimer: number | undefined;
  body.append(
    h("div", { class: "wiz-row" },
      field("wiz-name", t("agents.name"), name, w.errors.name),
      field("wiz-desc", t("wizard.description"), desc, w.errors.description),
    ),
    textArea("wiz-instr", t("agents.instructions"), d.instructions, LOCAL_AGENT_LIMITS.instructions, 9, {
      placeholder: t("agents.instructionsPlaceholder"),
      error: w.errors.instructions,
      onInput: (v) => {
        d.instructions = v;
        edited();
        window.clearTimeout(memoryTimer);
        memoryTimer = window.setTimeout(() => memory.update(d.instructions), MEMORY_WARN_DEBOUNCE_MS);
      },
    }),
    memory.el,
  );

  const prompts = h("div", { class: "field" }, h("label", { text: t("wizard.starterPrompts") }));
  const count = Math.min(LOCAL_AGENT_LIMITS.starterPrompts, Math.max(PROMPT_FIELDS, d.starterPrompts.length));
  for (let i = 0; i < count; i++) {
    const input = textInput(`wiz-prompt-${i}`, d.starterPrompts[i] ?? "", LOCAL_AGENT_LIMITS.starterPrompt, (v) => {
      const list = [...d.starterPrompts];
      while (list.length <= i) list.push("");
      list[i] = v;
      d.starterPrompts = list;
      edited();
    });
    input.placeholder = t("wizard.starterPlaceholder", { n: i + 1 });
    input.setAttribute("aria-label", t("wizard.starterPlaceholder", { n: i + 1 }));
    prompts.append(input);
  }
  prompts.append(fieldError(w.errors.starterPrompts));
  body.append(prompts);

  const web = switchEl(d.webSearch, false, t("agents.webSearch"), (v) => { d.webSearch = v; edited(); });
  body.append(h("div", { class: "row" }, web, h("span", { text: t("agents.webSearch") })));
}

function refineStep(w: Wizard, body: HTMLElement) {
  const d = w.draft ?? emptyDraft();
  body.append(h("p", { class: "hint", text: t("wizard.refineHint") }));
  body.append(textArea("wiz-feedback", t("wizard.feedback"), w.feedback, MAX_FEEDBACK, 3, {
    placeholder: t("wizard.feedbackPlaceholder"),
    autofocus: true,
    onInput: (v) => {
      w.feedback = v;
      const btn = dialog?.querySelector<HTMLButtonElement>("button[data-refine]");
      if (btn) btn.dataset.blocked = v.trim() && signedIn() ? "0" : "1";
      syncAiButtons();
    },
  }));

  const apply = h("button", {
    type: "button", class: "primary", "data-refine": "1", "data-needs-ai": "1",
    text: t("wizard.applyFeedback"), onclick: () => void runDraft("refine"),
  }) as HTMLButtonElement;
  apply.dataset.blocked = w.feedback.trim() && signedIn() ? "0" : "1";
  const undo = h("button", {
    type: "button", text: t("wizard.undo"), title: t("wizard.undoHint"), onclick: undoDraft,
  }) as HTMLButtonElement;
  undo.disabled = !w.undo.length || !!w.busy;
  const actions = h("div", { class: "row" });
  if (w.busy === "refine") {
    actions.append(h("button", { type: "button", class: "danger", text: t("chat.stop"), onclick: stopDraft }));
  } else {
    actions.append(apply);
  }
  actions.append(undo);
  body.append(actions);
  if (!signedIn()) body.append(h("div", { class: "notice warn", text: t("wizard.signedOut") }));
  const busy = busyLine(w);
  if (busy) body.append(busy);

  // What the agent currently is, read-only here (edit it on the Draft step).
  const prompts = d.starterPrompts.filter((p) => p.trim());
  body.append(h("div", { class: "wiz-summary" },
    h("div", { class: "agent-name", dir: "auto", text: d.name || t("wizard.unnamed") }),
    d.description ? h("div", { class: "hint", dir: "auto", text: d.description }) : null,
    h("div", { class: "wiz-instr", dir: "auto", text: d.instructions }),
    memoryWarning(d.instructions).el,
    prompts.length
      ? h("div", { class: "wiz-chips" }, ...prompts.map((p) => h("span", { class: "chip-s", dir: "auto", text: p })))
      : null,
  ));
}

/** A fake pill task, so the mini character is drawn exactly as on the island. */
function previewTask(color: string, name: string): AgentTask {
  return {
    id: "wizard-preview", name, color, state: "idle", stepIndex: 0, steps: [], source: "agent",
    isIntegration: true, emote: null,
  };
}

/** The island pill in miniature: the character in the agent's colour + its name. */
function pillPreview(color: string, name: string, big = false): HTMLElement {
  const pill = h("div", { class: big ? "pill-preview big" : "pill-preview", "aria-hidden": "true" },
    createMiniBot(previewTask(color, name), big ? 30 : 22),
    h("span", { class: "lbl", dir: "auto", text: name || t("wizard.unnamed") }),
  );
  pill.style.setProperty("--agent", color);
  return pill;
}

/**
 * Ticks the preview characters. The first frame runs once the step is in the
 * document, which is when canvases from the previous render can be dropped
 * (pruning earlier would drop the new ones too).
 */
function startPreviewLoop() {
  let last = performance.now();
  let first = true;
  const loop = (now: number) => {
    if (!wiz || (wiz.step !== 4 && wiz.step !== 5)) return;
    if (first) {
      pruneMiniBots();
      first = false;
    }
    tickMiniBots(Math.min(0.05, (now - last) / 1000));
    last = now;
    frame = requestAnimationFrame(loop);
  };
  frame = requestAnimationFrame(loop);
}

function lookStep(w: Wizard, body: HTMLElement) {
  const d = (w.draft ??= emptyDraft());
  const stage = h("div", { class: "wiz-stage" });
  const drawStage = () => {
    clear(stage);
    stage.append(pillPreview(w.color, d.name, true), pillPreview(w.color, d.name));
    if (stage.isConnected) pruneMiniBots();
  };
  const name = textInput("wiz-look-name", d.name, LOCAL_AGENT_LIMITS.name, (v) => {
    d.name = v;
    w.dirty = true;
    for (const lbl of stage.querySelectorAll(".lbl")) lbl.textContent = v || t("wizard.unnamed");
  });
  name.dataset.autofocus = "1";
  body.append(field("wiz-look-name", t("agents.name"), name, w.errors.name));

  const colors: string[] = [...AGENT_PALETTE];
  const suggested = isHexColor(d.suggestedColor) ? d.suggestedColor.toUpperCase() : null;
  if (suggested && !colors.includes(suggested)) colors.unshift(suggested);
  if (!colors.includes(w.color)) colors.unshift(w.color);
  const swatches = h("div", { class: "swatches", role: "radiogroup", "aria-label": t("wizard.color") });
  for (const c of colors) {
    const on = c === w.color;
    const label = c === suggested ? t("wizard.colorSuggested", { c }) : c;
    const sw = h("button", {
      type: "button", class: `swatch${on ? " on" : ""}${c === suggested ? " suggested" : ""}`, role: "radio",
      "aria-checked": on ? "true" : "false", "aria-label": label, title: label, style: `--c:${c}`,
    });
    sw.addEventListener("click", () => {
      w.color = c;
      w.dirty = true;
      for (const other of swatches.querySelectorAll<HTMLElement>(".swatch")) {
        const sel = other === sw;
        other.classList.toggle("on", sel);
        other.setAttribute("aria-checked", sel ? "true" : "false");
      }
      drawStage();
    });
    swatches.append(sw);
  }
  body.append(
    h("div", { class: "field" },
      h("label", { text: t("wizard.color") }),
      swatches,
      suggested ? h("div", { class: "hint swatch-note", text: t("wizard.colorSuggestedHint") }) : null,
      fieldError(w.errors.color),
    ),
    h("div", { class: "field" }, h("label", { text: t("wizard.preview") }), stage),
  );
  drawStage();
  startPreviewLoop();
}

function saveStep(w: Wizard, body: HTMLElement) {
  const d = w.draft ?? emptyDraft();
  const models = roadeepData().models ?? [];
  const model = w.model ? (models.find((m) => m.id === w.model)?.displayName ?? w.model) : t("agents.accountDefault");
  const prompts = d.starterPrompts.filter((p) => p.trim()).length;
  const meta = [model, d.webSearch ? t("agents.webBadge") : null, t("wizard.promptsCount", { n: prompts })]
    .filter((x): x is string => !!x);
  body.append(h("div", { class: "wiz-final" },
    pillPreview(w.color, d.name),
    h("div", { class: "agent-text" },
      h("div", { class: "agent-name", dir: "auto", text: d.name || t("wizard.unnamed") }),
      d.description ? h("div", { class: "hint", dir: "auto", text: d.description }) : null,
      h("div", { class: "hint", text: meta.join(" · ") }),
    ),
  ));
  const pill = w.editingId ? agentPillId("local", w.editingId) : null;
  const canShow = w.showPill || !pillsFull() || (pill != null && isPillOn(pill));
  const sw = switchEl(w.showPill && canShow, !canShow, t("wizard.showPill"), (v) => { w.showPill = v; });
  body.append(h("div", { class: "row" }, sw,
    h("div", { class: "feature-text" },
      h("span", { text: t("wizard.showPill") }),
      h("span", { class: "hint", text: canShow ? t("wizard.showPillHint") : t("pills.full", { max: MAX_ACTIVE_PILLS }) }),
    ),
  ));
  if (!canShow) w.showPill = false;
  startPreviewLoop();
}

function footer(w: Wizard): HTMLElement {
  const foot = h("div", { class: "wiz-foot" });
  if (w.confirmClose) {
    foot.append(
      h("span", { class: "confirm", role: "alert", text: t("wizard.discard") }),
      h("div", { class: "spacer" }),
      h("button", { type: "button", text: t("wizard.keepEditing"), "data-autofocus": "1", onclick: () => {
        w.confirmClose = false;
        render();
      } }),
      h("button", { type: "button", class: "danger", text: t("wizard.discardConfirm"), onclick: close }),
    );
    return foot;
  }
  const back = h("button", { type: "button", text: t("common.back"), onclick: () => goTo((w.step - 1) as Step) }) as HTMLButtonElement;
  back.disabled = w.step === 1 || !!w.busy;
  foot.append(back, h("div", { class: "spacer" }));

  const primary = (label: string, onClick: () => void, needsAi = false) => {
    const b = h("button", { type: "button", class: "primary", "data-primary": "1", text: label, onclick: onClick }) as HTMLButtonElement;
    if (needsAi) b.dataset.needsAi = "1";
    b.disabled = !!w.busy;
    return b;
  };

  switch (w.step) {
    case 1: {
      const manual = h("button", { type: "button", text: t("wizard.writeMyself"), onclick: () => {
        w.draft ??= emptyDraft();
        goTo(2);
      } }) as HTMLButtonElement;
      manual.disabled = !!w.busy;
      foot.append(manual);
      if (w.busy === "draft") {
        foot.append(h("button", { type: "button", class: "danger", text: t("chat.stop"), onclick: stopDraft }));
      } else {
        const draft = primary(t(w.draft ? "wizard.redraft" : "wizard.draftWithAi"), () => {
          if (!w.goal.trim()) {
            w.errors = { goal: t("wizard.goalRequired") };
            render();
            return;
          }
          w.errors = {};
          void runDraft("draft");
        }, true);
        // An empty goal says why on click; signed out, the notice above says it.
        draft.dataset.blocked = signedIn() ? "0" : "1";
        foot.append(draft);
      }
      break;
    }
    case 2:
      foot.append(primary(t("wizard.next"), () => {
        const errors = validate(w.draft ?? emptyDraft());
        delete errors.name;
        w.errors = errors;
        if (Object.keys(errors).length) render();
        else goTo(3);
      }));
      break;
    case 3:
      foot.append(primary(t("wizard.next"), () => goTo(4)));
      break;
    case 4:
      foot.append(primary(t("wizard.next"), () => {
        w.errors = validate(w.draft ?? emptyDraft());
        if (Object.keys(w.errors).length) {
          w.step = stepOf(w.errors);
          render();
        } else {
          goTo(5);
        }
      }));
      break;
    case 5:
      foot.append(primary(t(w.busy === "save" ? "wizard.saving" : w.editingId ? "common.save" : "agents.create"), () => void save()));
      break;
  }
  return foot;
}
