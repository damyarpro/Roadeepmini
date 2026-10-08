// Island views — DOM ports of IslandViewContent.swift. Paddings, font sizes,
// colours and wording are copied from the Swift views so both platforms read
// identically.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { Ticker } from "./ticker";
import { State, type AgentTask } from "../core/state";
import { washRGBA, type IslandViewName, type Wash } from "../core/layout";
import { createMiniBot, pruneMiniBots } from "../character/minibots";
import type { CelebrationKind } from "../character/engine";
import { buildPrompt } from "./chat";
import { buildChoose, buildUpload, buildUploading } from "./upload";
import { renderIntegrationCard, type IntegrationCardHooks } from "./integrations";
import { formatNumber, t } from "../core/i18n";
import { localizeError } from "../core/error-text";
import { PLANNER_VIEWS, buildPalette, openPalette } from "./palette";
import { icon as plannerIcon } from "./planner/ui";
import { buildTasks } from "./planner/tasks";
import { buildNotes } from "./planner/notes";
import { buildReminders } from "./planner/reminders";
import { buildHabits } from "./planner/habits";
import { buildToday } from "./planner/today";
import { buildWeek } from "./planner/week";
import { buildFocus } from "./planner/focus";
import { buildFire } from "./planner/fire";
import { buildActivity } from "./activity";
import {decideGlobalVoice,globalVoiceIndicator,globalVoiceSnapshot} from "../local/global-voice";
import {voiceApprovalCard} from "../local/live-ui";
import {withVoiceDraft} from "./planner/voice-draft";

export interface ViewActions {
  setView(v: IslandViewName): void;
  collapse(): void;
  setFocus(id: string): void;
  openTerminal(): void;
  /** The ↗ button: opens whatever the focused pill points at. */
  openTarget(): void;
  openUrl(url: string): void;
  decide(d: "allow" | "deny"): void;
  toggleSound(): void;
  setVolume(v: number): void;
  setAutoClose(seconds: number): void;
  openSettingsWindow(section?: string): void;
  blip(): void;
  /** The chat's maximise / restore button. */
  toggleMaximize(): void;
  /** A small burst from the character (task done, focus round over); honours Settings. */
  celebrate?(kind?: CelebrationKind): void;
}

export interface ViewHost {
  el: HTMLElement;
  sync(): void;
  /** Called when the view becomes active, for views with a text field. */
  focus?(): void;
  /** Called every frame while the view is on screen. */
  tick?(nowMs: number): void;
}

// ── Shared pieces ─────────────────────────────────────────────────────────────

function card(wash: Wash, ...children: (Node | string)[]): HTMLElement {
  const el = h("div", { class: wash ? "card wash" : "card" }, ...children);
  if (wash) el.style.setProperty("--wash", washRGBA(wash));
  return el;
}

function btn(
  label: string,
  kind: "primary" | "secondary",
  onClick: () => void,
  kbd?: string,
): HTMLElement {
  return h(
    "button",
    { class: `btn ${kind}`, onclick: onClick },
    h("span", { text: label }),
    kbd ? h("span", { class: "kbd", text: kbd }) : null,
  );
}

/** AgentWho — coloured dot + task name + grey label. */
function agentWho(task: AgentTask | null, label: string): HTMLElement {
  const row = h("div", { class: "who-row" });
  if (task) {
    row.append(dot(task.color, 8), h("span", { class: "n", dir: "auto", text: task.name }));
  }
  row.append(h("span", { text: label }));
  return row;
}

function stack(padLeft: number, padRight: number, ...children: Node[]): HTMLElement {
  const el = h("div", { class: "stack" }, ...children);
  el.style.padding = `4px ${padRight}px 4px ${padLeft}px`;
  return el;
}

// ── Header ────────────────────────────────────────────────────────────────────

export function buildHeader(actions: ViewActions): ViewHost {
  const tabHome = h("button", { class: "tab", title: t("header.overview"), onclick: () => go("overview") }, svg(ICONS.house, 13));
  const tabChat = h("button", { class: "tab", title: t("header.ask"), onclick: () => go("prompt") }, svg(ICONS.bubble, 13));
  const tabDrop = h("button", { class: "tab", title: t("header.drop"), onclick: () => go("upload") }, svg(ICONS.plus, 13));
  // The «/» menu: the planner and everything else, without typing.
  const tabMenu = h("button", {
    class: "tab", title: t("header.menu"), "aria-label": t("header.menu"),
    onclick: () => {
      actions.blip();
      openPalette(actions);
    },
  }, plannerIcon("menu", 13));

  const gearBtn = h("button", { title: t("header.settings"), onclick: () => go("settings") }, svg(ICONS.gear, 14));
  const soundBtn = h("button", { title: t("header.mute"), onclick: () => actions.toggleSound() }, svg(ICONS.speakerOn, 14));
  // Only on the chat (and its history): the large island is a chat mode.
  const maxBtn = h("button", { type: "button", class: "max-btn", onclick: () => actions.toggleMaximize() });
  let maxShown: boolean | null = null;

  function go(v: IslandViewName) {
    actions.blip();
    actions.setView(v);
  }

  const el = h(
    "div",
    { id: "header" },
    h("div", { class: "tabs" }, tabHome, tabChat, tabDrop, tabMenu),
    h("div", { class: "header-actions" }, maxBtn, globalVoiceIndicator(()=>actions.setView("prompt")), gearBtn, soundBtn),
  );

  return {
    el,
    sync() {
      const v = State.view;
      tabHome.classList.toggle("on", v === "overview" || v === "empty");
      tabChat.classList.toggle("on", v === "prompt");
      tabDrop.classList.toggle("on", v === "upload");
      tabMenu.classList.toggle("on", PLANNER_VIEWS.has(v));
      gearBtn.classList.toggle("on", v === "settings");
      clear(gearBtn);
      gearBtn.append(svg(v === "settings" ? ICONS.gearFill : ICONS.gear, 14));
      clear(soundBtn);
      soundBtn.append(svg(State.settings.soundEnabled ? ICONS.speakerOn : ICONS.speakerOff, 14));
      maxBtn.hidden = v !== "prompt";
      if (maxShown !== State.maximized) {
        maxShown = State.maximized;
        const label = t(State.maximized ? "header.restore" : "header.maximize");
        maxBtn.title = label;
        maxBtn.setAttribute("aria-label", label);
        maxBtn.setAttribute("aria-pressed", State.maximized ? "true" : "false");
        clear(maxBtn);
        maxBtn.append(svg(State.maximized ? ICONS.restore : ICONS.maximize, 14, { stroke: 1.8 }));
      }
      el.style.opacity = v === "confused" ? "0" : "1";
    },
  };
}

// ── Overview ──────────────────────────────────────────────────────────────────

export function buildOverview(actions: ViewActions): ViewHost {
  const ticker = new Ticker();
  const who = h("div", { class: "who" });
  const tickerBody = h("div", { class: "card-body" }, who, ticker.el);
  const leftBody = h("div", { class: "left-body" });
  const jump = h(
    "button",
    { class: "icon-btn jump", title: t("overview.open"), onclick: () => actions.openTarget() },
    svg(ICONS.arrowUpRight, 8),
  );
  const left = card(null, leftBody, jump);
  const pills = h("div", { class: "pills" });
  const right = card(null, pills);

  const el = h("div", { class: "view overview" },
    h("div", { class: "left" }, left),
    h("div", { class: "right" }, right),
  );

  let pillIds = "";
  let detailOpen = false;
  let lastFocus: string | null = null;
  let mode: "ticker" | "card" | null = null;
  let cardKey = "";

  const hooks: IntegrationCardHooks = {
    get detailOpen() {
      return detailOpen;
    },
    openDetail() {
      detailOpen = true;
      cardKey = "";
      State.notify();
    },
    closeDetail() {
      detailOpen = false;
      cardKey = "";
      State.notify();
    },
    openSettings: () => actions.openSettingsWindow(),
    openAgent: () => actions.openTarget(),
  };

  return {
    el,
    tick(nowMs: number) {
      if (mode === "ticker") ticker.tick(nowMs);
    },
    sync() {
      const task = State.focusTask;
      if (task?.id !== lastFocus) {
        lastFocus = task?.id ?? null;
        detailOpen = false;
        cardKey = "";
        mode = null;
      }

      // Coding observation has lifecycle evidence, never an integration API key.
      const sessionActive =
        task?.id === "integration_codex" || (task?.id === "integration_claude" && (task.state !== "idle" || task.steps.length > 0));

      if (task && sessionActive) {
        if (mode !== "ticker") {
          clear(leftBody);
          leftBody.append(tickerBody);
          mode = "ticker";
          cardKey = "";
        }
        clear(who);
        who.append(
          dot(task.color, 7),
          h("span", { class: "name", dir: "auto", text: task.name }),
          h("span", { class: "tool", text: task.id === "integration_codex" ? "Codex" : task.source === "claudeCode" ? "Claude Code" : "n8n" }),
        );
        if (task.id !== "integration_codex" && task.steps.length > 1) {
          who.append(h("span", {
            class: "count",
            text: `${formatNumber(Math.min(task.stepIndex + 1, task.steps.length))}/${formatNumber(task.steps.length)}`,
          }));
        }
        ticker.sync(task);
      } else if (task) {
        const info = State.integrations[task.id];
        const key = [
          task.id, task.name, task.color, detailOpen, task.state, task.steps.join("|"),
          info?.loaded, info?.error, info?.configured,
          JSON.stringify(info?.data ?? {}),
        ].join("~");
        if (key !== cardKey) {
          cardKey = key;
          mode = "card";
          clear(leftBody);
          leftBody.append(renderIntegrationCard(task, hooks));
        }
      }

      jump.style.display = detailOpen ? "none" : "";
      jump.title = t(task?.id === "integration_codex" ? "activity.openCodex" : "overview.open");
      jump.setAttribute("aria-label", jump.title);

      const others = State.otherTasks.slice(0, 4);
      const pillKey = others.map((t) => `${t.id}:${t.pillBadge ?? ""}:${t.name}:${t.color}`).join("|");
      if (pillKey !== pillIds) {
        pillIds = pillKey;
        clear(pills);
        for (const t of others) pills.append(buildPill(t, actions));
        pruneMiniBots();
      }
    },
  };
}

function buildPill(task: AgentTask, actions: ViewActions): HTMLElement {
  const label = task.id === "integration_claude" ? t("settings.appName") : task.name;
  const canvas = createMiniBot(task, 24);
  const pill = h(
    "div",
    { class: "pill", onclick: () => actions.setFocus(task.id) },
    canvas,
    h("span", { class: "lbl", dir: "auto", text: label }),
  );
  pill.style.borderColor = `${task.color}24`;
  pill.addEventListener("mouseenter", () => {
    pill.style.background = `${task.color}2e`;
    pill.style.borderColor = `${task.color}8c`;
    pill.style.boxShadow = `0 2px 10px ${task.color}59`;
    (pill.querySelector(".lbl") as HTMLElement).style.color = lighten(task.color, 0.3);
  });
  pill.addEventListener("mouseleave", () => {
    pill.style.background = "";
    pill.style.borderColor = `${task.color}24`;
    pill.style.boxShadow = "";
    (pill.querySelector(".lbl") as HTMLElement).style.color = "";
  });

  if (task.pillBadge) {
    const colors = { approval: "#F5A524", finished: "#22C55E", error: "#F4505E" } as const;
    const icons = { approval: ICONS.bang, finished: ICONS.check, error: ICONS.xmark } as const;
    const inner = h("i", { style: `background:${colors[task.pillBadge]}` }, svg(icons[task.pillBadge], 6, { stroke: task.pillBadge === "finished" ? 3 : 0 }));
    const badge = h("div", { class: "pill-badge" }, inner);
    badge.style.boxShadow = `0 0 4px ${colors[task.pillBadge]}99`;
    pill.append(badge);
  }
  return pill;
}

function lighten(hex: string, amount: number): string {
  const v = parseInt(hex.replace("#", ""), 16);
  const c = [(v >> 16) & 255, (v >> 8) & 255, v & 255].map((x) =>
    Math.min(255, Math.round(x + amount * 255)),
  );
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// ── Empty ─────────────────────────────────────────────────────────────────────

function buildEmpty(actions: ViewActions): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 118px;flex-direction:row;align-items:center;gap:16px" },
    h(
      "div",
      { style: "display:flex;flex-direction:column;gap:5px" },
      h("div", { class: "title", text: t("empty.title") }),
      h("div", { class: "sub", text: t("empty.sub") }),
    ),
    h("div", { class: "grow" }),
    btn(t("empty.ask"), "primary", () => actions.setView("prompt")),
  );
  return { el: h("div", { class: "view" }, card(null, body)), sync() {} };
}

// ── Approval ──────────────────────────────────────────────────────────────────

function buildApproval(actions: ViewActions): ViewHost {
  const who = h("div");
  const code = h("div", { class: "code" });
  const row = h("div", { class: "actions" });
  const el = h("div", { class: "view" }, card("amber", stack(116, 16, who, code, row)));
  let rowKey = "";
  return {
    el,
    sync() {
      clear(who);
      const task = State.pendingApproval?.taskId ? State.tasks.find(task => task.id === State.pendingApproval?.taskId) : State.focusTask;
      who.append(agentWho(task ?? null, t("approval.needsPermission")));
      // The whole point of approving here rather than in the terminal: this line
      // is the command, the file path or the URL being authorised, not just the
      // name of the tool asking.
      code.textContent = State.pendingApproval?.command || State.pendingApproval?.tool || "…";
      // Two buttons, built once. Rebuilding them between a mouse-down and a
      // mouse-up would swallow the click, and there is nothing left to vary:
      // "Always" is gone until the remembered-rules list exists to back it.
      if (rowKey === "built") return;
      rowKey = "built";
      clear(row);
      row.append(
        btn(t("approval.deny"), "secondary", () => actions.decide("deny"), "N"),
        btn(t("approval.allow"), "primary", () => actions.decide("allow"), "Y"),
      );
    },
  };
}

// ── Live voice approval ─────────────────────────────────────────────────────────

/** A change the live voice wants to make; approved by click or by the user's own "yes"/"no". */
function buildVoiceApproval(): ViewHost {
  const approval = voiceApprovalCard(decideGlobalVoice);
  approval.el.style.padding = "8px 16px 8px 116px";
  const el = h("div", { class: "view" }, card("amber", approval.el));
  return {
    el,
    // global-voice notifies once a second while an approval waits, so the countdown stays live.
    sync: () => approval.update(globalVoiceSnapshot()),
  };
}

// ── Question ──────────────────────────────────────────────────────────────────

function buildQuestion(): ViewHost {
  const who = h("div");
  // Claude's own words, in whatever language it used.
  const title = h("div", { class: "title", dir: "auto" });
  const row = h("div", { class: "actions" });
  const el = h("div", { class: "view" }, card("cyan", stack(116, 16, who, title, row)));
  return {
    el,
    sync() {
      clear(who);
      who.append(agentWho(State.focusTask, t("question.asking")));
      const task = State.focusTask;
      title.textContent = task?.steps.at(-1) ?? t("question.fallback");
      clear(row);
      row.append(h("div", { class: "sub", text: t("question.answerInTerminal") }));
    },
  };
}

// ── Error ─────────────────────────────────────────────────────────────────────

function buildError(actions: ViewActions): ViewHost {
  const who = h("div");
  const title = h("div", { class: "title", text: t("error.workflowStopped") });
  const detail = h("div", { class: "detail", dir: "auto" });
  const row = h("div", { class: "actions" },
    btn(t("error.retry"), "primary", () => actions.setView(State.defaultView())),
    btn(t("error.openN8n"), "secondary", () => actions.openUrl("")),
  );
  const el = h("div", { class: "view" }, card("red", stack(116, 16, who, title, detail, row)));
  return {
    el,
    sync() {
      const task = State.focusTask;
      clear(who);
      who.append(agentWho(task, task?.source === "n8n" ? "n8n" : "Claude Code"));
      title.textContent = task?.source === "n8n" ? t("error.workflowStopped") : t("error.sessionStopped");
      detail.textContent = localizeError(task?.steps.at(-1) ?? t("error.noDetail"));
      detail.title = detail.textContent;
    },
  };
}

// ── Finished ──────────────────────────────────────────────────────────────────

function buildFinished(actions: ViewActions): ViewHost {
  const who = h("div");
  const title = h("div", { class: "title", dir: "auto" });
  const row = h("div", { class: "actions" },
    btn(t("finished.openTerminal"), "primary", () => actions.openTerminal()),
    btn(t("common.ok"), "secondary", () => actions.collapse()),
  );
  const el = h("div", { class: "view" }, card("green", stack(116, 16, who, title, row)));
  return {
    el,
    sync() {
      clear(who);
      who.append(agentWho(State.focusTask, t("finished.label")));
      title.textContent = localizeError(State.focusTask?.steps.at(-1) ?? t("finished.fallback"));
    },
  };
}

// ── Confused ──────────────────────────────────────────────────────────────────

function buildConfused(): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 128px" },
    h("div", { class: "title", text: t("confused.title") }),
    h("div", { class: "sub", text: t("confused.sub") }),
  );
  return { el: h("div", { class: "view" }, card("pink", body)), sync() {} };
}

// ── Note ──────────────────────────────────────────────────────────────────────

function buildNote(): ViewHost {
  // Notes are mostly error messages passed through from Rust, often English.
  const title = h("div", { class: "title", dir: "auto" });
  const el = h("div", { class: "view" }, card(null, h("div", { class: "stack", style: "padding:0 18px 0 98px" }, title)));
  return {
    el,
    sync() {
      title.textContent = localizeError(State.noteMessage ?? "");
    },
  };
}

// ── In-island settings ────────────────────────────────────────────────────────

const AUTO_CLOSE_CHOICES = [2, 5, 10, 15, 30] as const;
/** Upper end of the volume slider; the sound engine's gain range. */
const VOLUME_MAX = 0.2;

function buildSettings(actions: ViewActions): ViewHost {
  // ── Row 1: sound ──
  const soundIcon = h("span", { class: "qs-ico" });
  const soundSwitch = h("button", {
    type: "button",
    class: "switch",
    role: "switch",
    "aria-label": t("islandSettings.sound"),
    onclick: () => actions.toggleSound(),
  });
  const volume = h("input", {
    type: "range",
    class: "qs-vol",
    min: "0",
    max: String(VOLUME_MAX),
    step: "0.005",
    "aria-label": t("islandSettings.volume"),
  }) as HTMLInputElement;
  const paintVolume = (v: number) => {
    volume.style.setProperty("--fill", `${Math.round((v / VOLUME_MAX) * 100)}%`);
    volume.setAttribute("aria-valuetext", formatNumber(v / VOLUME_MAX, { style: "percent", maximumFractionDigits: 0 }));
  };
  volume.addEventListener("input", () => {
    const v = Number(volume.value);
    paintVolume(v);
    actions.setVolume(v);
  });

  // ── Row 2: auto-close (radiogroup with roving tabindex) ──
  const radios = AUTO_CLOSE_CHOICES.map((s) =>
    h("button", {
      type: "button",
      role: "radio",
      "aria-label": t("islandSettings.secondsA11y", { n: s }),
      text: formatNumber(s),
      onclick: () => actions.setAutoClose(s),
    }),
  );
  const seg = h("div", { class: "qs-seg", role: "radiogroup", "aria-label": t("islandSettings.autoClose") }, ...radios);
  seg.addEventListener("keydown", (e) => {
    const i = radios.indexOf(e.target as HTMLButtonElement);
    if (i < 0) return;
    // Arrows follow what is on screen: in RTL the next choice sits to the left.
    const forward = getComputedStyle(seg).direction === "rtl" ? "ArrowLeft" : "ArrowRight";
    const back = forward === "ArrowLeft" ? "ArrowRight" : "ArrowLeft";
    let n: number;
    switch (e.key) {
      case forward: case "ArrowDown": n = i + 1; break;
      case back: case "ArrowUp": n = i - 1; break;
      case "Home": n = 0; break;
      case "End": n = radios.length - 1; break;
      default: return;
    }
    e.preventDefault();
    n = (n + radios.length) % radios.length;
    radios[n].focus();
    actions.setAutoClose(AUTO_CLOSE_CHOICES[n]);
  });

  // ── Row 3: account (icon + text, never colour alone) + settings ──
  // Signed in is plain status; signed out is the one action to fix it. The
  // settings window opens on its own page — the actions give no section hook.
  const signedInStatus = h("span", { class: "qs-chip ok" },
    h("span", { class: "qs-chip-ico" }, svg(ICONS.check, 10, { stroke: 3 })),
    h("span", { class: "qs-chip-text", text: t("islandSettings.roadeepOn") }),
  );
  const signInButton = h("button", {
    type: "button",
    class: "qs-chip qs-signin",
    onclick: () => actions.openSettingsWindow("account"),
  },
    h("span", { class: "qs-chip-ico" }, svg(ICONS.bang, 10)),
    h("span", { class: "qs-chip-text", text: t("islandSettings.signIn") }),
  );

  const label = (icon: Node, text: string) =>
    h("span", { class: "qs-label" }, icon, h("span", { class: "qs-label-text", text }));

  const rows = h(
    "div",
    { class: "qs" },
    h("div", { class: "qs-row" },
      label(soundIcon, t("islandSettings.sound")),
      h("div", { class: "qs-ctrl" }, soundSwitch, volume),
    ),
    h("div", { class: "qs-row" },
      label(h("span", { class: "qs-ico" }, svg(ICONS.timer, 14)), t("islandSettings.autoClose")),
      h("div", { class: "qs-ctrl" },
        h("span", { class: "qs-seg-wrap" }, seg, h("span", { class: "qs-unit", text: t("islandSettings.secondsUnit") })),
      ),
    ),
    h("div", { class: "qs-row qs-foot" },
      signedInStatus,
      signInButton,
      h("button", {
        type: "button",
        class: "qs-open",
        onclick: () => actions.openSettingsWindow(),
      }, svg(ICONS.gear, 14), h("span", { text: t("islandSettings.open") })),
    ),
  );

  const el = h("div", { class: "view" }, card(null, rows));
  let lastSound: boolean | null = null;

  return {
    el,
    sync() {
      const s = State.settings;

      soundSwitch.classList.toggle("on", s.soundEnabled);
      soundSwitch.setAttribute("aria-checked", String(s.soundEnabled));
      if (s.soundEnabled !== lastSound) {
        lastSound = s.soundEnabled;
        soundIcon.replaceChildren(svg(s.soundEnabled ? ICONS.speakerOn : ICONS.speakerOff, 14));
      }
      volume.disabled = !s.soundEnabled;
      if (document.activeElement !== volume) volume.value = String(s.soundVolume);
      paintVolume(s.soundVolume);

      const current = AUTO_CLOSE_CHOICES.findIndex((v) => v === s.autoCloseInterval);
      radios.forEach((b, i) => {
        const on = i === current;
        b.classList.toggle("on", on);
        b.setAttribute("aria-checked", String(on));
        // Roving tabindex: the checked radio (or the first, if none) takes Tab.
        b.tabIndex = i === (current < 0 ? 0 : current) ? 0 : -1;
      });

      const signedIn = State.roadeep.signedIn === true;
      signedInStatus.hidden = !signedIn;
      signInButton.hidden = signedIn;
    },
  };
}

// ── Placeholders filled in later stages ───────────────────────────────────────

function buildPlaceholder(title: string, sub: string): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 118px" },
    h("div", { class: "title", text: title }),
    h("div", { class: "sub", text: sub }),
  );
  return { el: h("div", { class: "view" }, card(null, body)), sync() {} };
}

// ── Registry ──────────────────────────────────────────────────────────────────

export function buildViews(
  actions: ViewActions,
  onChatHeightChange: () => void,
): Map<IslandViewName, ViewHost> {
  const map = new Map<IslandViewName, ViewHost>();
  map.set("activity", buildActivity());
  map.set("overview", buildOverview(actions));
  map.set("empty", buildEmpty(actions));
  map.set("approval", buildApproval(actions));
  map.set("question", buildQuestion());
  map.set("error", buildError(actions));
  map.set("finished", buildFinished(actions));
  map.set("confused", buildConfused());
  map.set("note", buildNote());
  map.set("settings", buildSettings(actions));
  map.set("prompt", buildPrompt(onChatHeightChange, (filter) => openPalette(actions, filter, "prompt")));
  map.set("upload", buildUpload());
  map.set("uploading", buildUploading());
  map.set("choose", buildChoose(actions));
  // Not in the Windows v1: sending a file by email, window attach + web result.
  map.set("mail", buildPlaceholder(t("placeholder.mail"), ""));
  map.set("searching", buildPlaceholder(t("placeholder.searching"), ""));
  map.set("result", buildPlaceholder(t("placeholder.result"), ""));
  map.set("menu", buildPalette(actions));
  map.set("tasks", buildTasks(actions));
  map.set("notes", buildNotes(actions));
  map.set("reminders", buildReminders(actions));
  map.set("habits", buildHabits(actions));
  map.set("today", buildToday(actions));
  map.set("week", buildWeek(actions));
  map.set("focus", buildFocus(actions));
  map.set("plannerFire", buildFire(actions));
  map.set("voiceApproval", buildVoiceApproval());
  // The live voice drafts planner changes in place, on the page they belong to.
  for (const view of ["tasks", "notes", "reminders", "habits", "today", "focus"] as const) {
    const host = map.get(view);
    if (host) map.set(view, withVoiceDraft(view, host, { snapshot: globalVoiceSnapshot, decide: decideGlobalVoice }));
  }
  return map;
}
