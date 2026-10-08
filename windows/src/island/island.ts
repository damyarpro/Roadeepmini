// The island: DOM shell, sizing animation, character placement, mouse handling.
// Mirrors IslandRootView.swift + IslandWindowController.swift.

import { Tracked, Spring, clamp } from "../core/anim";
import { Bridge, IS_TAURI, onDragDrop } from "../core/bridge";
import {
  EXPANDED_CORNER, EXPANDED_W, NOTCH_W,
  ROUNDED_CORNER, VIEW_LAYOUTS, botGlowColor, botGlowOpacity, botPosition, chatPromptHeight,
  islandSize, setChatMaxH,
  type BotEmoteName, type IslandMode, type IslandViewName,
} from "../core/layout";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import { Activity } from "../activity/store";
import { routeCodexTarget } from "../activity/codex-target";
import { requestActivitySession } from "../views/activity";
import { catalogOpenUrl, isWebUrl } from "../core/catalog";
import { BotEngine, effectiveEyeMotion, hexToRGB } from "../character/engine";
import { Greeting } from "../character/greeting";
import { createMiniBot, pruneMiniBots, syncMiniBotStates, tickMiniBots } from "../character/minibots";
import { UploadCanvas } from "../upload/canvas";
import { USC, UploadSeq } from "../upload/sequence";
import { buildHeader, buildViews, type ViewActions, type ViewHost } from "../views/views";
import { chatWantedHeight } from "../views/chat";
import { h } from "../views/dom";
import { IslandStateMachine } from "./fsm";
import { initRoadeepSession } from "./roadeep";
import { t } from "../core/i18n";
import { localizeError } from "../core/error-text";
import type { DockDragFrame } from "../core/bridge";
import {
  DEFAULT_LAYOUT, HIT_MARGIN, canDragFrom, cornerRadii, dockedSize, dragEffect, dragTransform,
  islandRectFor, layoutShift, shoulderSize, sideBotPlacement, wakeStripRect,
  type DockEdge, type DockLayout,
} from "./dock";
import { devDockFromUrl, devLayout, devLineDelay, devView, mountDevScreen } from "./dev-preview";
import { createFocusChip, focusRunning } from "../views/planner/focus";
import { initPlannerAlerts } from "../views/planner/fire";
import { Planner } from "../views/planner/store";
import { holdsLine, lineDelaySeconds } from "../core/idle";
import { preserveActivityView } from "../views/activity";
import {bindGlobalVoiceIsland,globalVoiceActive,globalVoiceIndicator,globalVoiceRemembered} from "../local/global-voice";
import {approvalCleared} from "../local/approval-focus";
import { HooksBridge, answersFit } from "../core/bridge-hooks";
import { BridgeShortcuts } from "../core/bridge-shortcuts";
import { cardView, endApproval, pendingQuestions } from "./approvals";

const BOT_OVERHANG = 40;
/** The question card with options to pick from grows by this much: room for two rows of them. */
const QUESTION_PICKER_EXTRA_H = 40;
/** A press that moves this far (px) becomes a drag of the island. */
const DRAG_THRESHOLD = 6;

/** The three views the drop sequence owns; leaving them stops the engine. */
const UPLOAD_VIEWS: ReadonlySet<IslandViewName> = new Set(["upload", "uploading", "choose"]);
/**
 * Views the user opened to type or work in: the only ones that let the island
 * take keyboard focus. Alerts (the planner's included) never do.
 */
const KEYBOARD_VIEWS: ReadonlySet<IslandViewName> = new Set([
  "activity",
  "prompt", "menu", "tasks", "notes", "reminders", "habits", "today", "week", "focus",
]);

/** Seconds between the drop and the moment the progress bar starts filling. */
const PRE_PROGRESS = USC.T_PROG_START - USC.T_DROP;

const modeOrder = (m: IslandMode) => (m === "hidden" ? 0 : m === "compact" ? 1 : 2);

const reducedMotion = () => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

/** Resolves after `n` animation frames (the page has painted what came before). */
const frames = (n: number) =>
  new Promise<void>((resolve) => {
    const step = () => (--n <= 0 ? resolve() : requestAnimationFrame(step));
    requestAnimationFrame(step);
  });

export class Island {
  readonly fsm = new IslandStateMachine();

  private root: HTMLElement;
  private islandEl!: HTMLElement;
  private clipEl!: HTMLElement;
  private contentEl!: HTMLElement;
  private viewsEl!: HTMLElement;
  private botCanvas!: HTMLCanvasElement;
  private botGlow!: HTMLElement;
  private greetingCanvas!: HTMLCanvasElement;
  private miniGrid!: HTMLElement;
  private countdown!: HTMLElement;
  /** The compact island's focus timer (views/planner/focus.ts). */
  private focusChip = createFocusChip();
  private voiceChip = globalVoiceIndicator();
  private voiceWasActive = false;
  private wakeStrip!: HTMLElement;

  private header!: ViewHost;
  private views!: Map<IslandViewName, ViewHost>;
  private actions!: ViewActions;
  private uploadCanvas!: UploadCanvas;

  private width = new Tracked(NOTCH_W);
  private height = new Tracked(0);
  private radius = new Tracked(ROUNDED_CORNER);

  /** Dock edge and the window layout Rust sent for it (src/island/dock.ts). */
  private edge: DockEdge;
  private layout: DockLayout;
  /** Plain-browser stand-in for the native window (dev-preview.ts). */
  private dev: ReturnType<typeof mountDevScreen> | null = null;
  /** Plain-browser preview only: `?idle=N` seconds before the line, instead of the setting. */
  private devLineDelay: number | null = null;
  /** 1 while docked, 0 while floating in a drag: squares the edge-side corners, grows the shoulders. */
  private attach = new Spring(1, 0.4, 0.75);
  /** Drag squash/stretch (a vector along the motion) and tilt, springy so they wobble out. */
  private fxX = new Spring(0, 0.32, 0.42);
  private fxY = new Spring(0, 0.32, 0.42);
  private fxTilt = new Spring(0, 0.36, 0.45);
  /** A left press that may turn into a click (compact island) or a drag. */
  private press: { x: number; y: number; click: boolean; drag: boolean } | null = null;
  /** A drag in progress (Rust moves the window); `resume` reopens an expanded island after it. */
  private drag: { resume: { view: IslandViewName; pinned: boolean } | null } | null = null;
  private botCx = new Spring(46);
  private botCy = new Spring(16);
  private botSize = new Spring(10);

  private engine = new BotEngine();
  private greeting = new Greeting();

  private running = false;
  private lastFrame = 0;
  private dirty = true;
  private canvasPx = 0;

  // Rust starts the window at full size so the launch greeting has room.
  private collapsed = false;
  private collapseTimer: number | null = null;
  private wasInIsland = false;
  /** Last shape handed to Rust for the click-through test. */
  private pushedRect = { x: -1, y: -1, w: -1, h: -1 };
  /** Context menu shape; it takes the mouse together with the island while open. */
  private menuRect: { x: number; y: number; w: number; h: number } | null = null;

  // Bot hover → love (IslandWindowController.botHoverIn)
  private botHovering = false;
  private botHoverTimer: number | null = null;
  private lastLoveTime = 0;
  private botHoverStart = { x: 0, y: 0 };

  private confusedRecovery: number | null = null;
  private prevViewBeforeConfused: IslandViewName = "overview";
  private lastSyncedView: IslandViewName | null = null;

  /** Drop sequence bookkeeping: last tick played, and whether the ✓ has fired. */
  private uploadTens = 0;
  private uploadDone = false;

  constructor(root: HTMLElement, layout: DockLayout | null) {
    this.root = root;
    if (!IS_TAURI) {
      const dock = devDockFromUrl(location.search, { w: window.innerWidth, h: window.innerHeight });
      layout = devLayout(dock);
      this.dev = mountDevScreen(root, dock);
      this.devLineDelay = devLineDelay(location.search);
      State.settings.dock = { edge: dock.edge, pos: dock.pos, monitor: "" };
    }
    this.layout = layout ?? DEFAULT_LAYOUT;
    this.edge = this.layout.edge;
    setChatMaxH(this.layout.chatMaxH);
    this.build();
    this.wireFsm();
    this.wireInput();
    this.engine.onDizzy = () => this.handleDizzy();
    this.greeting.onComplete = () => this.fsm.greetComplete();
    State.subscribe(() => {
      const voiceActive=globalVoiceActive();
      if(voiceActive!==this.voiceWasActive){this.voiceWasActive=voiceActive;if(voiceActive)this.fsm.reveal();this.animateGeometry(!voiceActive);}
      this.dirty = true;
      this.ensureRunning();
    });
    void initRoadeepSession();
    initPlannerAlerts({
      alert: (view) => this.alert(view),
      collapse: () => this.collapse(),
      dropPin: () => this.dropPin(),
      inDropFlow: () => this.inDropFlow,
    }, this.actions);
    bindGlobalVoiceIsland({ alert: (view) => this.alert(view), setView: (view) => this.setView(view), dropPin: () => this.dropPin(), reveal: () => this.reveal() });
  }

  // ── DOM ─────────────────────────────────────────────────────────────────────

  private build() {
    const actions: ViewActions = {
      setView: (v) => this.setView(v),
      collapse: () => this.collapse(),
      foldApproval: () => this.foldApproval(),
      setFocus: (id) => {
        State.setFocus(id);
        Sound.play("blip");
        // A pill with a waiting request opens on its card: going back to it
        // after looking at another pill brings the card up again.
        const card = cardView();
        if (card && State.pendingApproval?.taskId === id) {
          this.setView(card);
          return;
        }
        // An agent pill is a shortcut to chatting with that agent.
        const ref = State.focusTask?.agentRef;
        if (ref) this.openAgentChat(ref);
      },
      openTerminal: () => {
        const cwd = State.focusTask?.sessionCwd ?? null;
        void BridgeShortcuts.openSession(State.focusTask?.sessionId ?? null, cwd);
      },
      // The ↗ button — same targets as openAgentTarget() on macOS.
      openTarget: () => {
        const task = State.focusTask;
        if (!task) return;
        if (routeCodexTarget(task.id, Activity.sessions(), {
          select: requestActivitySession,
          openActivity: () => this.setView("activity"),
        })) return;
        if (task.agentRef) {
          this.openAgentChat(task.agentRef);
          return;
        }
        const urls: Record<string, string> = {
          integration_resend: "https://resend.com/emails",
          integration_vercel: "https://vercel.com/dashboard",
          integration_github: "https://github.com",
          integration_stripe: "https://dashboard.stripe.com/payments",
          integration_notion: "https://notion.so",
          integration_calcom: "https://app.cal.com/bookings",
          integration_linear: "https://linear.app",
          integration_netlify: "https://app.netlify.com",
        };
        // GitLab, Sentry, Cloudflare and catalog services live at a user-chosen
        // instance, region or account; their poller sends the page to open along
        // with the data. Before the first answer a catalog service falls back to
        // its manifest's fixed page.
        const fromData = State.integrations[task.id]?.data?.openUrl;
        const fallback = task.isCatalog ? catalogOpenUrl(task.id) : null;
        if (task.id === "integration_claude") void BridgeShortcuts.openSession(task.sessionId ?? null, task.sessionCwd ?? null);
        else if (task.id === "integration_n8n") void Bridge.openN8n();
        else if (urls[task.id]) void Bridge.openUrl(urls[task.id]);
        else if (isWebUrl(fromData)) void Bridge.openUrl(fromData);
        else if (fallback) void Bridge.openUrl(fallback);
      },
      openUrl: (url) => {
        if (url) void Bridge.openUrl(url);
      },
      decide: (d) => {
        const req = State.pendingApproval;
        void Bridge.log(`decide ${d} req=${req?.requestId ?? "none"}`);
        if (!req) return;
        Sound.play(d === "deny" ? "blip" : "approve");
        void Bridge.approvalDecision(req.requestId, d);
        this.closeApproval();
      },
      answer: (answers) => {
        const req = State.pendingApproval;
        const questions = pendingQuestions();
        if (!req || !questions) return;
        if (answersFit(questions, answers)) {
          Sound.play("approve");
          void HooksBridge.approvalAnswer(req.requestId, answers);
        } else {
          // Never send what does not answer the questions as asked: the
          // terminal takes the request back instead.
          void Bridge.log(`island: answer did not fit req=${req.requestId}, handed back to the terminal`);
          Sound.play("blip");
          void Bridge.approvalDecline(req.requestId);
        }
        this.closeApproval();
      },
      answerInTerminal: () => {
        const req = State.pendingApproval;
        if (!req) return;
        Sound.play("blip");
        void Bridge.approvalDecline(req.requestId);
        this.closeApproval();
      },
      toggleSound: () => {
        State.settings.soundEnabled = !State.settings.soundEnabled;
        Sound.setEnabled(State.settings.soundEnabled);
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      setVolume: (v) => {
        State.settings.soundVolume = v;
        Sound.setVolume(v);
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      setAutoClose: (s) => {
        State.settings.autoCloseInterval = s;
        // A countdown already running starts again with the new delay.
        this.fsm.homeToPetitDelay = s;
        void Bridge.saveSettings(State.settings);
        State.notify();
      },
      openSettingsWindow: (section?: string) => void Bridge.openSettingsWindow(section),
      blip: () => Sound.play("blip"),
      toggleMaximize: () => this.toggleMaximize(),
      celebrate: (kind = "task") => {
        // Unseen, a burst would only wait frozen and play on the next reveal.
        if (State.mode === "hidden") return;
        this.engine.celebrate(kind);
        this.ensureRunning();
      },
    };

    this.wakeStrip = h("div", { id: "wake-strip" });
    this.botGlow = h("div", { id: "bot-glow" });
    this.botCanvas = h("canvas", { id: "bot-canvas" });
    this.greetingCanvas = h("canvas", { id: "greeting-canvas" });
    this.miniGrid = h("div", { id: "mini-grid" });
    this.countdown = h("div", { id: "countdown" });

    this.actions = actions;
    this.viewsEl = h("div", { id: "views" });
    this.contentEl = h("div", { id: "content" });
    this.buildContent();

    // The drop sequence draws the card, the bar and its own character. It sits under
    // the header, which stays visible on top of it exactly as on macOS.
    this.uploadCanvas = new UploadCanvas({
      ask: () => {
        State.promptContext = State.droppedFile
          ? { kind: "file", name: State.droppedFile.name, path: State.droppedFile.path }
          : null;
        this.setView("prompt");
      },
      cancel: () => this.setView(State.defaultView()),
    });

    this.clipEl = h(
      "div",
      { id: "island-clip" },
      this.greetingCanvas,
      this.uploadCanvas.el,
      this.contentEl,
    );
    this.islandEl = h(
      "div",
      { id: "island" },
      this.clipEl,
      this.botGlow,
      this.botCanvas,
      this.miniGrid,
      this.countdown,
      this.focusChip.el,
      this.voiceChip,
      globalVoiceRemembered(),
    );

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    this.greetingCanvas.width = Math.round(EXPANDED_W * dpr);
    this.greetingCanvas.height = Math.round(150 * dpr);
    this.greetingCanvas.style.width = `${EXPANDED_W}px`;
    this.greetingCanvas.style.height = "150px";

    this.islandEl.dataset.edge = this.edge;
    this.root.append(this.wakeStrip, this.islandEl);
    this.placeWakeStrip();
    this.dev?.place(this.layout, State.maximized);
    this.applyGeometry();
  }

  /**
   * Header and views carry their wording from the moment they are built, so a
   * language change rebuilds them. Everything they show lives in State, except
   * a half-typed chat message.
   */
  private buildContent() {
    this.header = buildHeader(this.actions);
    this.views = buildViews(this.actions, () => this.animateGeometry(false));
    this.viewsEl.replaceChildren(...[...this.views.values()].map((v) => v.el));
    this.contentEl.replaceChildren(this.header.el, this.viewsEl);
  }

  /** Re-renders every view in the current language, without a restart. */
  relocalize() {
    preserveActivityView();
    this.buildContent();
    pruneMiniBots();
    this.lastSyncedView = null;
    this.dirty = true;
    this.ensureRunning();
  }

  // ── FSM ─────────────────────────────────────────────────────────────────────

  private wireFsm() {
    this.fsm.homeToPetitDelay = State.settings.autoCloseInterval;
    this.fsm.lineDelay = this.lineDelay();
    this.fsm.holdLine = () => this.lineHeld();
    this.fsm.onTransition = (from, to) => {
      switch (to) {
        case "hidden":
          this.setMode("hidden");
          break;
        case "petit":
          if (from === "greeting") this.greeting.interrupt();
          else if (from === "hidden") { Sound.play("peek"); this.engine.arrive(); }
          this.setMode("compact");
          if (from === "greeting") State.view = State.defaultView();
          if (!this.wasInIsland) this.fsm.mouseLeft();
          break;
        case "home": {
          // A card waiting for an answer comes first: opening a folded island
          // shows it again (Mac #117, #290), and the mouse never folds it.
          const card = cardView();
          if (card) {
            State.isPinned = true;
            this.fsm.pinned = true;
          }
          this.expand(card ?? State.defaultView());
          if (!this.wasInIsland) this.fsm.mouseLeft();
          break;
        }
        case "greeting":
          this.expand("greeting");
          this.greeting.start();
          break;
      }
      State.notify();
    };
  }

  launch() {
    this.fsm.launch();
    // Plain-browser preview only: `?view=tasks` opens a view once the greeting is over.
    const view = this.dev ? devView(location.search) : null;
    if (view) {
      window.setTimeout(() => {
        State.isPinned = true;
        this.alert(view);
      }, 2600);
    }
  }

  /** Seconds before the untouched compact island becomes the line; 0 = never. */
  private lineDelay(): number {
    return this.devLineDelay ?? lineDelaySeconds(State.settings.absenceInterval);
  }

  /** Something still needs the compact island (core/idle.ts holdsLine). */
  private lineHeld(): boolean {
    // A running focus timer keeps its compact chip on screen too.
    return holdsLine(State, { dragging: this.drag != null, menuOpen: this.menuRect != null, uploading: UploadSeq.isActive }) ||
      focusRunning(Planner.data?.focus) || globalVoiceActive();
  }

  // ── Mode / view ─────────────────────────────────────────────────────────────

  private setMode(mode: IslandMode) {
    const prev = State.mode;
    if (mode === prev) return;
    State.mode = mode;
    // Hidden, the island is the line on its edge (static CSS on the wake strip).
    this.wakeStrip.classList.toggle("line", mode === "hidden");
    if (mode === "expanded") Sound.play("open");
    if (prev === "expanded") {
      Sound.play("close");
      // A folded card is still waiting: it keeps the island pinned.
      if (!State.pendingApproval) State.isPinned = false;
      void Bridge.focusWindow(false);
    }
    if (mode !== "expanded") {
      this.setMaximized(false);
      this.engine.resetMorph();
      // Nothing can be seen of the sequence once the island is shut, and leaving
      // it running would keep the frame loop awake — the island must cost
      // nothing while hidden.
      UploadSeq.deactivate();
    }
    this.updateWindowCollapsed();
    this.animateGeometry(modeOrder(mode) < modeOrder(prev));
    State.notify();
  }

  /**
   * True from the moment a file is dragged in until the user has said what to
   * do with it: work events (a Claude Code turn ending, an integration update)
   * must not replace these views, or the drop would seem to do nothing.
   */
  get inDropFlow(): boolean {
    return UploadSeq.isActive || (State.mode === "expanded" && UPLOAD_VIEWS.has(State.view));
  }

  /** True while the drop sequence owns the island body. */
  private get uploadActive(): boolean {
    return State.mode === "expanded" && UploadSeq.isActive && UPLOAD_VIEWS.has(State.view);
  }

  /** Navigating out of the drop flow ends the sequence, as on macOS. */
  private stopSequenceIfLeaving(view: IslandViewName) {
    if (UploadSeq.isActive && !UPLOAD_VIEWS.has(view)) UploadSeq.deactivate();
  }

  expand(view: IslandViewName) {
    view = this.cardOrView(view);
    this.stopSequenceIfLeaving(view);
    if (view !== "prompt") this.setMaximized(false);
    State.view = view;
    if (State.mode !== "expanded") this.setMode("expanded");
    else this.animateGeometry(false);
    State.lastActivity = performance.now();
    State.notify();
  }

  /**
   * The permission card asked for while the waiting request is one of Claude
   * Code's questions: the question card, never Deny / Allow for a question.
   */
  private cardOrView(view: IslandViewName): IslandViewName {
    return view === "approval" ? cardView() ?? view : view;
  }

  setView(view: IslandViewName) {
    view = this.cardOrView(view);
    this.stopSequenceIfLeaving(view);
    const wasMaximized = State.maximized;
    if (view !== "prompt") this.setMaximized(false);
    if (State.mode !== "expanded") {
      this.fsm.forceHome();
      State.view = view;
      this.animateGeometry(false);
      State.notify();
      return;
    }
    const grew = !wasMaximized && VIEW_LAYOUTS[view].height >= VIEW_LAYOUTS[State.view].height;
    State.view = view;
    State.lastActivity = performance.now();
    this.animateGeometry(!grew);
    State.notify();
  }

  /**
   * Opens the chat with an agent selected (a Roadeep id or "local:<id>"). A
   * different agent is a different conversation, so the thread starts over.
   */
  openAgentChat(ref: string) {
    if (State.settings.chatAgent !== ref) {
      State.settings.chatAgent = ref;
      State.chatHistory = [];
      void Bridge.chatReset();
      void Bridge.saveSettings(State.settings);
      void Bridge.log("island: agent pill selected a chat agent");
    }
    this.setView("prompt");
  }

  collapse() {
    // A waiting card is only ever folded, never dropped by a close.
    if (State.pendingApproval && State.mode === "expanded") {
      this.foldApproval();
      return;
    }
    this.setMaximized(false);
    State.isPinned = false;
    this.fsm.pinned = false;
    // Drive the state machine rather than the mode: setting the mode behind its
    // back left it thinking the island was still open, and a click on the compact
    // island then did nothing — the island could never be reopened.
    this.fsm.forcePetit();
  }

  /** Alert from the hook server: open on this view. Pinned alerts never auto-close. */
  alert(view: IslandViewName) {
    this.fsm.pinned = State.isPinned || State.maximized;
    this.fsm.forceHome();
    this.expand(view);
  }

  /**
   * Folds a card that is waiting for an answer down to the compact island,
   * without answering it (Mac #290). Nothing is decided: the request keeps
   * waiting, the island stays on screen, and opening it shows the card again.
   */
  foldApproval() {
    if (!State.pendingApproval || State.mode !== "expanded") return;
    State.isPinned = true;
    this.fsm.pinned = true;
    this.fsm.forcePetit();
  }

  /** The request has its answer: the card goes and the session carries on. */
  private closeApproval() {
    endApproval();
    this.cardCleared();
  }

  /**
   * The card was answered or withdrawn (hooks.ts): a live-voice approval
   * waiting behind it comes back; otherwise the pin goes and the island leaves
   * the card for its usual view. A folded card goes quietly: the compact
   * island is not opened just to say it is gone.
   */
  cardCleared() {
    const onCard = State.view === "approval" || State.view === "question";
    if (onCard && State.mode !== "expanded") State.view = State.defaultView();
    approvalCleared(this, "approval");
    if (!State.pendingApproval && State.mode === "expanded" && State.view === "question") {
      this.setView(State.defaultView());
    }
  }

  /**
   * Opens the chat and puts the caret in its field (the global shortcut). When
   * the chat is already the view — the island had only folded away — syncDom
   * sees no switch and would not focus it, so it is done here.
   */
  focusChat() {
    const alreadyChat = this.lastSyncedView === "prompt";
    this.alert("prompt");
    if (!alreadyChat) return;
    void Bridge.focusWindow(true);
    window.setTimeout(() => this.views.get("prompt")?.focus?.(), 120);
  }

  reveal() {
    this.fsm.reveal();
  }

  /** The character reacts to a shortcut (Ctrl+Alt+1..7). */
  emote(name: BotEmoteName) {
    this.engine.triggerEmote(name);
    this.ensureRunning();
  }

  /** Ctrl+P: keep the open island from folding away, or let it fold again. */
  setPinned(on: boolean) {
    State.isPinned = on;
    this.fsm.pinned = on || State.maximized;
    if (on) this.fsm.cancelAutoClose();
    else if (this.fsm.state === "home" && !State.maximized && !this.wasInIsland) this.fsm.mouseLeft();
    State.notify();
  }

  /** An alert stopped waiting for an answer: let the island auto-close again. */
  dropPin() {
    this.fsm.pinned = State.maximized;
    // The countdown the pin held back starts now, if the mouse is elsewhere.
    if (!this.wasInIsland && !this.drag && (this.fsm.state === "home" || this.fsm.state === "petit")) {
      this.fsm.mouseLeft();
    }
  }

  // ── Maximised chat ──────────────────────────────────────────────────────────

  /** The header button: the chat becomes a large island, or goes back. */
  toggleMaximize() {
    if (State.mode !== "expanded" || State.view !== "prompt") return;
    this.setMaximized(!State.maximized);
  }

  /**
   * A large chat island, still docked. Only the button and "Minimize now"
   * bring it back (auto-close, leaving it and Esc don't); leaving the chat or
   * the island folding restores it first. Not remembered across launches.
   * Rust slides the window so the large island fits on screen.
   */
  private setMaximized(on: boolean) {
    if (State.maximized === on || (on && this.drag)) return;
    State.maximized = on;
    this.fsm.pinned = on || State.isPinned;
    if (on) {
      this.fsm.cancelAutoClose();
    } else if (this.fsm.state === "home" && !this.wasInIsland && !this.drag) {
      // Back to normal with the cursor elsewhere: the usual auto-close applies again.
      this.fsm.mouseLeft();
    }
    this.islandEl.classList.toggle("maximized", on);
    void Bridge.setMaximized(on, reducedMotion());
    this.dev?.place(this.layout, on);
    this.animateGeometry(!on);
    void Bridge.log(`island: chat ${on ? "maximised" : "restored"}`);
    State.notify();
  }

  // ── Dock ────────────────────────────────────────────────────────────────────

  get dockEdge(): DockEdge {
    return this.edge;
  }

  /** A new layout from Rust (display change, new dock, the window reopening). */
  setLayout(next: DockLayout) {
    const prev = this.layout;
    this.layout = next;
    this.edge = next.edge;
    setChatMaxH(next.chatMaxH);
    this.islandEl.dataset.edge = next.edge;
    this.placeWakeStrip();
    this.dev?.place(next, State.maximized);
    const sized = prev.edge !== next.edge || prev.chatMaxH !== next.chatMaxH ||
      prev.maxW !== next.maxW || prev.maxH !== next.maxH;
    if (sized) this.animateGeometry(false);
    this.ensureRunning();
  }

  /**
   * A drop (or a dock change from the menu): Rust is about to spring the
   * window into the new dock. When the layout changes (edge or window size)
   * the island is hidden for the two frames it takes to re-lay it out and for
   * Rust to shift the window by the opposite amount, so it is never seen in
   * two places.
   */
  async onDockMove(p: { layout: DockLayout; wait: boolean }) {
    this.islandEl.removeAttribute("data-preview");
    // A new dock always starts at the normal size (Rust has dropped maximised too).
    this.setMaximized(false);
    const next = p.layout;
    const relaid = next.edge !== this.edge || next.panelW !== this.layout.panelW || next.panelH !== this.layout.panelH;
    if (!relaid) {
      this.setLayout(next);
      if (p.wait) void Bridge.dockLayoutReady(0, 0);
      return;
    }
    const { dx, dy } = layoutShift(this.layout, next, this.width.value, this.height.value);
    this.islandEl.style.visibility = "hidden";
    this.setLayout(next);
    this.applyGeometry();
    try {
      await frames(2);
      if (p.wait) await Bridge.dockLayoutReady(dx, dy);
      await frames(1);
    } finally {
      this.islandEl.style.visibility = "";
    }
  }

  /** One frame of a drag or of the spring after it: squash, tilt, edge preview. */
  onDockDrag(f: DockDragFrame) {
    if (!this.drag) return;
    if (!f.settling) this.islandEl.dataset.preview = f.edge;
    if (reducedMotion()) return;
    const fx = dragEffect(f.vx, f.vy);
    this.fxX.target = fx.sx;
    this.fxY.target = fx.sy;
    this.fxTilt.target = fx.tilt;
    this.ensureRunning();
  }

  /** The island is in its dock: it re-attaches and reopens if it was open. */
  onDockSettled() {
    const d = this.drag;
    this.drag = null;
    this.islandEl.removeAttribute("data-preview");
    this.fxX.target = 0;
    this.fxY.target = 0;
    this.fxTilt.target = 0;
    if (reducedMotion()) this.attach.set(1);
    else this.attach.target = 1;
    // Hover is measured afresh from where the island landed.
    this.wasInIsland = false;
    if (d?.resume) {
      this.fsm.forceHome();
      this.expand(d.resume.view);
      State.isPinned = d.resume.pinned;
      this.fsm.pinned = d.resume.pinned;
    } else if (this.fsm.state === "petit") {
      // The drop counts as the last touch: the idle count starts here, and the
      // next cursor event over the island stops it again (hover is measured afresh).
      this.fsm.mouseLeft();
    }
    this.ensureRunning();
  }

  /** Drags start from the compact island, or the expanded one's header and margins. */
  private get canDrag(): boolean {
    return IS_TAURI && !this.drag && !State.maximized && !this.collapsed && State.mode !== "hidden" &&
      !State.fileDragOver && !UploadSeq.isActive && !UPLOAD_VIEWS.has(State.view) && State.view !== "greeting";
  }

  /**
   * The press moved far enough: the island turns into the compact pill and
   * Rust moves the window with the cursor until the button is released.
   */
  private startDrag() {
    if (!this.canDrag) return;
    const resume = State.mode === "expanded" ? { view: State.view, pinned: State.isPinned } : null;
    this.drag = { resume };
    this.cancelBotHover();
    if (resume) this.fsm.forcePetit();
    if (reducedMotion()) this.attach.set(0);
    else this.attach.target = 0;
    this.ensureRunning();
    const pill = dockedSize(this.edge, "compact", islandSize("compact", State.view,0,null,globalVoiceActive()));
    const r = islandRectFor(this.edge, this.layout, pill.w, pill.h);
    void Bridge.log("island: drag started");
    void Bridge.dragStart(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, r.h / 2, reducedMotion()).then((ok) => {
      if (!ok) this.onDockSettled();
    });
  }

  // ── File drop ───────────────────────────────────────────────────────────────

  private onDragDrop(e: { type: string; paths?: string[] }) {
    if (e.type !== "over") void Bridge.log(`drag ${e.type} ${e.paths?.length ?? 0} file(s)`);
    if (State.paused) return;
    switch (e.type) {
      case "enter":
      case "over": {
        if (State.fileDragOver) return;
        State.fileDragOver = true;
        this.engine.animateMorph(1);
        // enterZone must run before the island expands, so the sequence is
        // already active by the time the view becomes `upload`.
        UploadSeq.enterZone(State.mouseInIsland.x, State.mouseInIsland.y);
        this.alert("upload");
        break;
      }
      case "leave": {
        if (!State.fileDragOver) return;
        State.fileDragOver = false;
        this.engine.animateMorph(0);
        // The island deliberately stays open: the drag session is still alive.
        UploadSeq.exitZone();
        State.notify();
        break;
      }
      case "drop": {
        State.fileDragOver = false;
        const path = e.paths?.[0];
        if (!path) {
          this.engine.animateMorph(0);
          this.setView(State.defaultView());
          return;
        }
        this.swallow(path);
        break;
      }
    }
  }

  /**
   * The character eats the file. Nothing here waits on the file system: the copy into
   * the inbox runs in the background and swaps the path in when it lands, so a
   * slow disk can never stall the animation — same as FileDropHandler on macOS.
   */
  private swallow(path: string) {
    const name = path.split(/[\\/]/).pop() || t("common.file");
    State.droppedFile = { name, path };
    State.promptContext = { kind: "file", name, path };
    State.chatHistory = [];
    void Bridge.chatReset();

    // A quick drag can land without a preceding `enter` (the window takes the
    // mouse only once the cursor is near the island): the sequence must still
    // start, or the island would sit on the uploading card for ever.
    if (!UploadSeq.isActive) {
      this.engine.animateMorph(1);
      UploadSeq.enterZone(State.mouseInIsland.x, State.mouseInIsland.y);
    }
    UploadSeq.performDrop(State.uploadDuration);
    this.uploadTens = 0;
    this.uploadDone = false;

    this.engine.gulp();
    Sound.play("approve");
    this.engine.triggerEmote("happy");
    this.engine.animateMorph(0);

    State.uploadProgress = 0;
    this.setView("uploading");
    this.ensureRunning();

    void Bridge.ingestFile(path)
      .then((file) => {
        State.droppedFile = { name: file.name, path: file.path };
        State.promptContext = { kind: "file", name: file.name, path: file.path };
        State.notify();
      })
      .catch((err) => {
        UploadSeq.deactivate();
        // Rust sends a coded string (E_FILE_IS_FOLDER…): shown in the UI language.
        State.noteMessage = localizeError(err);
        this.engine.animateMorph(0);
        this.setView("note");
        Sound.play("error");
        window.setTimeout(() => this.setView(State.defaultView()), 2400);
      });
  }

  /**
   * Sounds and view changes hung off the canvas timeline: a `tick` every 10 %,
   * the ✓ chime when the bar completes, then `choose` once the character has grown back.
   */
  private stepSequence() {
    const since = UploadSeq.sinceDrop();
    if (since == null) return;
    const dur = State.uploadDuration;
    const p = Math.max(0, Math.min(1, (since - PRE_PROGRESS) / dur));

    // The HTML card's bar and percentage (shown when the canvas is not).
    State.uploadProgress = p;

    const tens = Math.floor(p * 10);
    if (tens > this.uploadTens && tens < 10) {
      this.uploadTens = tens;
      Sound.play("tick");
    }

    if (!this.uploadDone && since >= PRE_PROGRESS + dur) {
      this.uploadDone = true;
      Sound.play("approve");
      this.engine.triggerEmote("happy");
    }
    // The extra second is the grow-back, after which the choose card is up.
    if (since >= PRE_PROGRESS + dur + 1 && State.view === "uploading") {
      this.setView("choose");
    }
  }

  // ── Geometry ────────────────────────────────────────────────────────────────

  private targetSize(): { w: number; h: number; r: number } {
    let size = islandSize(State.mode, State.view, State.chatHistory.length, chatWantedHeight(),globalVoiceActive());
    if (State.maximized && State.mode === "expanded" && State.view === "prompt") {
      size = { w: this.layout.maxW, h: this.layout.maxH };
    }
    if (State.mode === "expanded" && State.view === "question" && pendingQuestions()) {
      size = { w: size.w, h: size.h + QUESTION_PICKER_EXTRA_H };
    }
    const { w, h } = dockedSize(this.edge, State.mode, size);
    const r = State.mode === "expanded" ? EXPANDED_CORNER : ROUNDED_CORNER;
    return { w, h, r };
  }

  private animateGeometry(shrinking: boolean) {
    const { w, h, r } = this.targetSize();
    if (reducedMotion() || State.mode === "hidden" && this.collapsed) {
      // No spring, no curve: the island takes its new size at once.
      this.width.jump(w);
      this.height.jump(h);
      this.radius.jump(r);
    } else if (shrinking) {
      this.width.curveTowards(w);
      this.height.curveTowards(h);
      this.radius.curveTowards(r);
    } else {
      this.width.springTo(w);
      this.height.springTo(h);
      this.radius.springTo(r);
    }
    this.ensureRunning();
  }

  private applyGeometry() {
    const w = this.width.value;
    const hh = this.height.value;
    const r = this.radius.value;
    const attach = this.attach.value;
    const island = islandRectFor(this.edge, this.layout, w, hh);
    const style = this.islandEl.style;
    style.left = `${island.x}px`;
    style.top = `${island.y}px`;
    style.width = `${w}px`;
    style.height = `${hh}px`;
    style.borderRadius = cornerRadii(this.edge, r, attach);
    // The outward shoulders where the island meets the edge shrink away with
    // it, and while it floats in a drag.
    style.setProperty("--shoulder", `${shoulderSize(this.edge, w, hh, attach)}px`);
    style.transform = dragTransform(this.fxX.value, this.fxY.value, this.fxTilt.value);
    // These follow the island as it resizes, so they belong here rather than in
    // the state-driven DOM sync. A standing (side-docked) compact island has its
    // character on top and the mini grid at the bottom.
    if (hh > w) {
      this.miniGrid.style.left = `${w / 2 - 14.5}px`;
      this.miniGrid.style.top = `${hh - 40 - 14.5}px`;
    } else {
      this.miniGrid.style.left = `${w - 40 - 14.5}px`;
      this.miniGrid.style.top = `${hh / 2 - 14.5}px`;
    }
    const voiceStyle=this.voiceChip.style;
    if(hh>w){voiceStyle.insetInlineEnd="auto";voiceStyle.left="0";voiceStyle.top="48px";voiceStyle.width=`${w}px`;voiceStyle.height="auto";voiceStyle.flexDirection="column";}
    else{voiceStyle.insetInlineEnd="12px";voiceStyle.left="auto";voiceStyle.top="0";voiceStyle.width="auto";voiceStyle.height="48px";voiceStyle.flexDirection="row";}
    // The focus chip sits beside the compact character: to its right, or under it standing.
    const chip = this.focusChip.el.style;
    if (hh > w) {
      chip.left = `${w / 2}px`;
      chip.top = "58px";
      chip.transform = "translateX(-50%)";
    } else {
      chip.left = "58px";
      chip.top = `${hh / 2}px`;
      chip.transform = "translateY(-50%)";
    }
    this.greetingCanvas.style.left = `${(w - EXPANDED_W) / 2}px`;
    this.uploadCanvas.el.style.left = `${(w - EXPANDED_W) / 2}px`;

    let rect = { x: island.x, y: island.y, w, h: hh };
    const m = this.menuRect;
    if (m) {
      // Rust only knows one rectangle, so the menu and the island share their bounding box.
      const x0 = Math.min(rect.x, m.x);
      const y0 = Math.min(rect.y, m.y);
      rect = {
        x: x0,
        y: y0,
        w: Math.max(rect.x + rect.w, m.x + m.w) - x0,
        h: Math.max(rect.y + rect.h, m.y + m.h) - y0,
      };
    }
    const p = this.pushedRect;
    if (
      Math.abs(p.x - rect.x) > 0.5 || Math.abs(p.y - rect.y) > 0.5 ||
      Math.abs(p.w - rect.w) > 0.5 || Math.abs(p.h - rect.h) > 0.5
    ) {
      this.pushedRect = rect;
      void Bridge.setIslandRect(rect.x, rect.y, rect.w, rect.h);
    }
  }

  /** Island rect in window coordinates (origin top-left of the window). */
  private islandRect(): { x: number; y: number; w: number; h: number } {
    return islandRectFor(this.edge, this.layout, this.width.value, this.height.value);
  }

  /**
   * The wake strip: while the window is collapsed it IS the window; otherwise
   * it lies along the island's edge.
   */
  private placeWakeStrip() {
    this.wakeStrip.dataset.edge = this.edge;
    const s = this.wakeStrip.style;
    if (IS_TAURI && this.collapsed) {
      s.left = "0";
      s.top = "0";
      s.width = "100%";
      s.height = "100%";
      return;
    }
    const r = wakeStripRect(this.edge, this.layout);
    s.left = `${r.x}px`;
    s.top = `${r.y}px`;
    s.width = `${r.w}px`;
    s.height = `${r.h}px`;
  }

  // ── Context menu ────────────────────────────────────────────────────────────

  /** False while the window is (or is about to be) the hidden wake strip. */
  get canShowMenu(): boolean {
    return State.mode !== "hidden" && !this.collapsed;
  }

  /**
   * The context menu opened (rect) or closed (null). While it is open the
   * window takes the mouse over it, counts it as part of the island (so the
   * island doesn't auto-close under it) and takes keyboard focus, so the menu
   * can be driven by keys and a click elsewhere closes it through `blur`.
   */
  setMenuRect(rect: { x: number; y: number; w: number; h: number } | null) {
    this.menuRect = rect;
    this.applyGeometry();
    if (rect) {
      void Bridge.focusWindow(true);
    } else if (!(State.mode === "expanded" && KEYBOARD_VIEWS.has(State.view))) {
      // Only the views with text fields keep keyboard focus, as in syncDom.
      void Bridge.focusWindow(false);
    }
  }

  // ── Window collapse (hidden → tiny wake strip, zero polling) ────────────────

  private updateWindowCollapsed() {
    if (this.collapseTimer != null) {
      window.clearTimeout(this.collapseTimer);
      this.collapseTimer = null;
    }
    if (State.mode === "hidden") {
      // Let the island finish retracting, then drop the window to the wake strip:
      // from there the OS delivers no cursor events, so nothing polls at all.
      this.collapseTimer = window.setTimeout(() => {
        this.collapseTimer = null;
        if (State.mode !== "hidden") return;
        this.collapsed = true;
        this.placeWakeStrip();
        void Bridge.setCollapsed(true);
      }, 420);
    } else if (this.collapsed) {
      // Grow the window back before the island animates open.
      this.collapsed = false;
      this.placeWakeStrip();
      void Bridge.setCollapsed(false);
    }
  }

  // ── Input ───────────────────────────────────────────────────────────────────

  private wireInput() {
    // The wake strip is the only thing the OS can hit while the island is hidden.
    this.wakeStrip.addEventListener("mouseenter", () => {
      Sound.resume();
      if (State.mode === "hidden") this.fsm.mouseEntered();
    });

    this.islandEl.addEventListener("mousedown", (e) => {
      // A right click only opens the context menu: it must not open or slap.
      if (e.button !== 0) return;
      Sound.resume();
      State.lastActivity = performance.now();
      // The compact island opens on release, so a press that moves can drag it instead.
      if (State.mode !== "expanded") {
        this.press = { x: e.clientX, y: e.clientY, click: true, drag: this.canDrag };
        e.preventDefault();
        return;
      }
      const at = this.toWindow(e.clientX, e.clientY);
      if (this.isBotHit(at.x, at.y)) {
        this.cancelBotHover();
        this.engine.slap();
        this.ensureRunning();
        return;
      }
      if (this.canDrag && canDragFrom(e.target, this.islandEl)) {
        this.press = { x: e.clientX, y: e.clientY, click: false, drag: true };
        // No text selection starts from a drag handle.
        e.preventDefault();
      }
    });
    window.addEventListener("mousemove", (e) => {
      const p = this.press;
      if (!p || !p.drag || Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
      this.press = null;
      this.startDrag();
    });
    window.addEventListener("mouseup", (e) => {
      if (e.button !== 0) return;
      const p = this.press;
      this.press = null;
      if (p?.click && !this.drag) this.fsm.click();
    });

    window.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && State.mode === "expanded") {
        // Only keys typed into the island itself land here, never Escape typed
        // in a terminal — so it may fold a waiting card away, as Escape in the
        // notch does on macOS.
        if (State.pendingApproval && (State.view === "approval" || State.view === "question")) this.foldApproval();
        // Maximised, the chat only goes back with its button or "Minimize now".
        else if (!State.isPinned && !State.maximized) this.collapse();
      }
      State.lastActivity = performance.now();
    });

    void onDragDrop((e) => this.onDragDrop(e));

    // Outside Tauri (plain browser) drive the cursor from DOM events so the
    // island can be inspected with `npm run dev`.
    if (!IS_TAURI) {
      window.addEventListener("mousemove", (e) => {
        const at = this.toWindow(e.clientX, e.clientY);
        this.onCursor(at.x, at.y);
      });
    }
  }

  /** Page coordinates → window coordinates (they differ only in the browser preview). */
  private toWindow(x: number, y: number): { x: number; y: number } {
    return this.dev ? this.dev.toWindow(x, y) : { x, y };
  }

  /** Cursor in window-logical coordinates. */
  onCursor(x: number, y: number) {
    State.mouse = { x, y };
    const rect = this.islandRect();
    State.mouseInIsland = { x: x - rect.x, y: y - rect.y };

    // Windows sends no cursor position with an OLE drag, so the drop sequence is
    // fed from the Win32 cursor poll instead — it runs throughout the drag.
    if (UploadSeq.isActive && !UploadSeq.dropped) {
      UploadSeq.updateCursor(State.mouseInIsland.x, State.mouseInIsland.y);
    }

    const within = (r: { x: number; y: number; w: number; h: number }) =>
      x >= r.x - HIT_MARGIN && x <= r.x + r.w + HIT_MARGIN &&
      y >= r.y - HIT_MARGIN && y <= r.y + r.h + HIT_MARGIN;
    // While dragged the island is under the cursor by definition: no leave timers.
    const inIsland = this.drag != null || within(rect) || (this.menuRect != null && within(this.menuRect));

    if (inIsland && !this.wasInIsland) {
      if (this.fsm.state === "greeting") this.greeting.hover();
      this.fsm.mouseEntered();
    }
    if (!inIsland && this.wasInIsland) {
      this.fsm.mouseLeft();
    }
    this.wasInIsland = inIsland;

    // Bot hover → love
    const overBot = State.mode === "expanded" && !this.drag && State.stateOverride == null && this.isBotHit(x, y);
    if (overBot && !this.botHovering) this.botHoverIn(x, y);
    if (!overBot && this.botHovering) this.cancelBotHover();
    this.botHovering = overBot;
    if (this.botHovering) {
      const d = Math.hypot(x - this.botHoverStart.x, y - this.botHoverStart.y);
      if (d > 40) {
        this.botHoverStart = { x, y };
        this.scheduleLove();
      }
    }

    this.ensureRunning();
  }

  private isBotHit(x: number, y: number): boolean {
    const rect = this.islandRect();
    const cx = rect.x + this.botCx.value;
    const cy = rect.y + this.botCy.value;
    const radius = this.botSize.value / 2;
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius * radius;
  }

  private botHoverIn(x: number, y: number) {
    if (performance.now() / 1000 - this.lastLoveTime < 6) return;
    this.botHoverStart = { x, y };
    this.engine.blink();
    this.engine.tgEs = 1.08;
    Sound.play("hover");
    this.scheduleLove();
  }

  private scheduleLove() {
    if (this.botHoverTimer != null) window.clearTimeout(this.botHoverTimer);
    this.botHoverTimer = window.setTimeout(() => {
      this.botHoverTimer = null;
      if (!this.botHovering || State.stateOverride != null) return;
      if (performance.now() / 1000 - this.lastLoveTime < 6) return;
      this.lastLoveTime = performance.now() / 1000;
      this.engine.triggerEmote("love");
      this.ensureRunning();
      Sound.play("love");
    }, 1900);
  }

  private cancelBotHover() {
    if (this.botHoverTimer != null) window.clearTimeout(this.botHoverTimer);
    this.botHoverTimer = null;
    this.engine.tgEs = 1;
  }

  /** Three slaps → dizzy + confused view for 3.3 s, then back. */
  private handleDizzy() {
    this.prevViewBeforeConfused = State.view;
    State.stateOverride = "dizzy";
    this.engine.setState("dizzy");
    Sound.play("dizzy");
    this.alert("confused");
    if (this.confusedRecovery != null) window.clearTimeout(this.confusedRecovery);
    this.confusedRecovery = window.setTimeout(() => {
      this.confusedRecovery = null;
      State.stateOverride = null;
      this.engine.setState(State.effectiveState);
      if (State.view === "confused") {
        const fallback = State.defaultView();
        this.setView(this.prevViewBeforeConfused === "confused" ? fallback : this.prevViewBeforeConfused);
      }
      this.engine.triggerEmote("happy");
      this.ensureRunning();
    }, 3300);
  }

  // ── Frame loop ──────────────────────────────────────────────────────────────

  ensureRunning() {
    if (this.running) return;
    this.running = true;
    this.lastFrame = performance.now();
    requestAnimationFrame(this.frame);
  }

  /**
   * One frame. A throw inside it must not end the loop for good: `running`
   * would stay true, ensureRunning would do nothing, and every animation —
   * the character, the drop sequence, the chat — would freeze until a restart.
   */
  private frame = (nowMs: number) => {
    try {
      this.step(nowMs);
    } catch (err) {
      const text = String(err).slice(0, 300);
      if (text !== this.lastFrameError) {
        this.lastFrameError = text;
        console.error("[roadeep] frame failed", err);
        void Bridge.log(`island: frame failed: ${text}`);
      }
      // Not rescheduled from here: a throw that repeats every frame would spin
      // at 60 fps, hidden island included. The next ensureRunning() — any
      // input, event or view change — starts the loop again.
      this.running = false;
    }
  };

  private lastFrameError = "";

  private step = (nowMs: number) => {
    const dt = Math.min(0.05, (nowMs - this.lastFrame) / 1000);
    this.lastFrame = nowMs;

    this.width.step(dt, nowMs);
    this.height.step(dt, nowMs);
    this.radius.step(dt, nowMs);
    this.attach.step(dt);
    this.fxX.step(dt);
    this.fxY.step(dt);
    this.fxTilt.step(dt);
    this.applyGeometry();

    if (this.dirty) {
      this.dirty = false;
      this.syncDom();
    }

    this.updateBotTargets();
    this.botCx.step(dt);
    this.botCy.step(dt);
    this.botSize.step(dt);

    const greetingActive = State.mode === "expanded" && State.view === "greeting";
    if (greetingActive) {
      const gctx = this.greetingCanvas.getContext("2d");
      if (gctx) {
        const dpr = Math.min(2, window.devicePixelRatio || 1);
        gctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        this.greeting.draw(gctx);
      }
    } else {
      // Kept running even while the drop canvas is up, so the island's own character
      // is already in the right place the moment the canvas fades out.
      this.drawBot(dt);
    }

    const uploadActive = this.uploadActive;
    if (uploadActive) this.uploadCanvas.draw(UploadSeq.frame(), nowMs / 1000);
    this.uploadCanvas.el.classList.toggle("on", uploadActive);
    this.viewsEl.classList.toggle("hidden-by-upload", uploadActive);

    tickMiniBots(dt);
    // A ticker scroll that loses its frames freezes mid-way, rows overlapping.
    const viewAnimating = this.views.get(State.view)?.tick?.(nowMs) === true;
    if (UploadSeq.isActive) this.stepSequence();
    this.updateCountdown(nowMs);

    // Nothing is drawn while the island is hidden, so nothing may keep the loop
    // alive either. This used to read `... || this.engine.busy || State.mode !==
    // "hidden"`, and engine.busy is permanently true for any state with a
    // looping animation — breathing, ratelimit sweat, sleeping z's, the search
    // sweep — so a hidden island went on burning frames in exactly the states it
    // spends most of its life in. Geometry still has to finish retracting.
    const settling =
      this.width.animating || this.height.animating || this.radius.animating ||
      !this.attach.settled || !this.fxX.settled || !this.fxY.settled || !this.fxTilt.settled;
    const busy = State.mode === "hidden"
      ? settling
      : settling ||
        !this.botCx.settled || !this.botCy.settled || !this.botSize.settled ||
        greetingActive || this.engine.busy || UploadSeq.isActive || viewAnimating;

    if (busy) {
      requestAnimationFrame(this.frame);
    } else {
      this.running = false;
      Sound.idle();
    }
  };

  private updateBotTargets() {
    const p = sideBotPlacement(this.edge, State.mode,
      botPosition(State.mode, State.view, this.height.value, State.uploadProgress));
    this.botCx.target = p.cx;
    this.botCy.target = p.cy;
    this.botSize.target = p.diameter / 0.6;

    const greetingActive = State.mode === "expanded" && State.view === "greeting";
    // The drop canvas draws its own character; two of them would overlap.
    const visible = p.opacity > 0 && !greetingActive && !this.uploadActive;
    this.botCanvas.style.opacity = visible ? "1" : "0";

    if (State.mode === "expanded" && State.view !== "uploading" && !greetingActive && !this.uploadActive) {
      const d = p.diameter;
      const color = botGlowColor(State.effectiveState);
      this.botGlow.style.display = "block";
      this.botGlow.style.width = `${d * 2.2}px`;
      this.botGlow.style.height = `${d * 2.2}px`;
      this.botGlow.style.left = `${this.botCx.value - d * 1.1}px`;
      this.botGlow.style.top = `${this.botCy.value - d * 1.1}px`;
      this.botGlow.style.background = `radial-gradient(circle, ${color} 0%, transparent 62%)`;
      this.botGlow.style.opacity = String(botGlowOpacity(State.effectiveState));
    } else {
      this.botGlow.style.display = "none";
    }
  }

  private drawBot(dt: number) {
    const size = this.botSize.value;
    const w = Math.max(1, Math.round(size));
    const hCss = w + BOT_OVERHANG;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    if (this.canvasPx !== w) {
      this.canvasPx = w;
      this.botCanvas.width = Math.round(w * dpr);
      this.botCanvas.height = Math.round(hCss * dpr);
      this.botCanvas.style.width = `${w}px`;
      this.botCanvas.style.height = `${hCss}px`;
    }
    this.botCanvas.style.left = `${this.botCx.value - w / 2}px`;
    this.botCanvas.style.top = `${this.botCy.value - BOT_OVERHANG / 2 - hCss / 2}px`;

    const ctx = this.botCanvas.getContext("2d");
    if (!ctx) return;

    const focus = State.focusTask;
    this.engine.bodyColor = focus?.isIntegration ? hexToRGB(focus.color) : null;
    this.engine.particleOverhang = BOT_OVERHANG;
    this.engine.lookX = this.lookX();
    this.engine.lookY = this.lookY();
    if (this.engine.morph > 0.3) {
      this.engine.slotHTarget = State.fileDragOver ? 0.2 : 0;
    } else {
      this.engine.slotHTarget = 0;
      if (this.engine.morph < 0.05) {
        this.engine.slotH = 0;
        this.engine.slotHVel = 0;
      }
    }
    this.engine.update(dt);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, hCss);
    this.engine.draw(ctx, w, hCss);
  }

  /** BotCanvasView.lookX / lookY — tanh of the distance to the bot. */
  private lookX(): number {
    const rect = this.islandRect();
    const botScreenX = rect.x + this.botCx.value;
    return Math.tanh((State.mouse.x - botScreenX) / 260);
  }

  private lookY(): number {
    const rect = this.islandRect();
    return -Math.tanh((State.mouse.y - rect.y - this.botCy.value) / 200);
  }

  private updateCountdown(nowMs: number) {
    // The state machine's own deadline, so the bar follows an auto-close delay
    // edited while the countdown runs.
    const dueAt = this.fsm.homeCollapseDueAt;
    if (State.mode !== "expanded" || State.isPinned || dueAt == null) {
      this.countdown.style.width = "0px";
      return;
    }
    const autoClose = this.fsm.homeToPetitDelay;
    const windowS = Math.min(10, autoClose * 0.6);
    const remaining = (dueAt - nowMs) / 1000;
    this.countdown.style.width =
      remaining < windowS ? `${Math.max(0, clamp(remaining / windowS, 0, 1) * 160)}px` : "0px";
  }

  // ── DOM sync ────────────────────────────────────────────────────────────────

  private syncDom() {
    const expanded = State.mode === "expanded";
    const greetingActive = expanded && State.view === "greeting";

    this.contentEl.style.opacity = expanded && !greetingActive ? "1" : "0";
    this.contentEl.style.pointerEvents = expanded && !greetingActive ? "auto" : "none";
    // Views nobody can see keep no animation running (views/island-cards.css).
    this.viewsEl.classList.toggle("asleep", !expanded || greetingActive);
    this.greetingCanvas.style.display = greetingActive ? "block" : "none";

    this.voiceChip.classList.add("compact-voice-chip");
    this.voiceChip.hidden=State.mode!=="compact"||!globalVoiceActive();
    this.header.sync();
    for (const [name, view] of this.views) {
      const on = name === State.view;
      view.el.classList.toggle("on", on);
      view.el.inert = !on || !expanded;
      view.el.setAttribute("aria-hidden", String(!on || !expanded));
      if (on) view.sync();
    }

    if (this.lastSyncedView !== State.view) {
      const hadKeyboard = this.lastSyncedView != null && KEYBOARD_VIEWS.has(this.lastSyncedView);
      this.lastSyncedView = State.view;
      if (KEYBOARD_VIEWS.has(State.view)) {
        const view = State.view;
        void Bridge.focusWindow(true);
        window.setTimeout(() => this.views.get(view)?.focus?.(), 120);
      } else if (hadKeyboard) {
        void Bridge.focusWindow(false);
      }
    }

    // Compact mini grid
    this.focusChip.update();
    if(globalVoiceActive()&&State.mode==="compact")this.focusChip.el.classList.remove("on");

    const showGrid = State.mode === "compact";
    this.miniGrid.style.opacity = showGrid ? "1" : "0";
    if (showGrid) {
      const others = State.otherTasks.slice(0, 4);
      const key = others.map((t) => t.id).join("|");
      if (this.miniGrid.dataset.key !== key) {
        this.miniGrid.dataset.key = key;
        this.miniGrid.replaceChildren();
        for (const t of others) {
          this.miniGrid.append(createMiniBot(t, 13));
        }
        pruneMiniBots();
      }
    }

    syncMiniBotStates(State.tasks);
    this.engine.setState(State.effectiveState);
  }

  /** Applies settings coming from Rust at boot. */
  applySettings() {
    BotEngine.setAppearance(State.settings.characterAppearance);
    Sound.setEnabled(State.settings.soundEnabled);
    Sound.setVolume(State.settings.soundVolume);
    this.fsm.homeToPetitDelay = State.settings.autoCloseInterval;
    this.fsm.setLineDelay(this.lineDelay());
    // The OS reduced-motion preference is read here, at boot and on every settings change.
    const reduced = reducedMotion();
    BotEngine.setEyeMotion(effectiveEyeMotion(State.settings.eyeMotion ?? "normal", reduced));
    BotEngine.setCelebrations(State.settings.celebrations ?? true, reduced);
    State.notify();
  }

  get panelSize() {
    return { w: this.layout.panelW, h: this.layout.panelH };
  }

  get chatHeight() {
    return chatPromptHeight(State.chatHistory.length, chatWantedHeight());
  }
}
