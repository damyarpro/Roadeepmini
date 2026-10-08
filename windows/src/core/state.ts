// App state — mirror of AppState.swift (the parts the island needs).

import type { BotEmoteName, BotStateName, IslandMode, IslandViewName } from "./layout";
import type { EyeMotion, EyeShape } from "../character/engine";
import { DEFAULT_LANGUAGE, type Language } from "./i18n";
import type { LocalAgent, RoadeepAgent, RoadeepUser } from "./bridge";
import { catalogEntryOf } from "./catalog";
import { DEFAULT_APPEARANCE, type CharacterAppearance } from "../character/appearance";

export type AgentSource = "claudeCode" | "n8n" | "agent";
export type PillBadge = "approval" | "finished" | "error";

export interface AgentTask {
  id: string;
  name: string;
  color: string;
  state: BotStateName;
  stepIndex: number;
  steps: string[];
  /** Observed, bounded step windows can move without changing stepIndex. */
  stepRevision?: string;
  stepSessionId?: string;
  directHook?: boolean;
  source: AgentSource;
  isIntegration: boolean;
  emote?: BotEmoteName | null;
  miniEye?: EyeShape | null;
  pillBadge?: PillBadge | null;
  sessionCwd?: string | null;
  /** Agent pills only: the settings.chatAgent value its click selects. */
  agentRef?: string | null;
  /** A service run from a catalog manifest (core/catalog.ts). */
  isCatalog?: boolean;
}

export interface ApprovalInfo {
  taskId?: string;
  provider?: string;
  requestId: string;
  sessionId: string;
  tool: string;
  command: string;
}

export interface ChatMessage {
  origin?: "local" | "cloud" | "cpu" | "app";
  routeReason?: string;
  id: number;
  role: "user" | "assistant";
  content: string;
}

export type PromptContext =
  | { kind: "window"; appName: string; title: string; url?: string }
  | { kind: "file"; name: string; path?: string };

export interface ResultItem {
  label: string;
  detail: string;
  url?: string;
}

export interface SearchResult {
  title: string;
  items: ResultItem[];
  note?: string;
}

const task = (
  id: string, name: string, color: string, source: AgentSource,
): AgentTask => ({
  id, name, color, state: "idle", stepIndex: 0, steps: [], source, isIntegration: true,
});

/**
 * AgentTask.integrationAgents — same ids, names and colours as macOS. GitLab,
 * Sentry, Linear, Netlify and Cloudflare are Windows-only; their colours are
 * picked away from the existing hues (Cloudflare's own orange would read as n8n).
 */
export const INTEGRATION_AGENTS: AgentTask[] = [
  task("integration_claude", "Roadeep", "#F5F6F8", "claudeCode"),
  task("integration_resend", "Resend", "#22C55E", "n8n"),
  task("integration_n8n", "n8n", "#F29B38", "n8n"),
  task("integration_vercel", "Vercel", "#7C5CFF", "n8n"),
  task("integration_github", "GitHub", "#F4505E", "n8n"),
  task("integration_notion", "Notion", "#8C8C8C", "n8n"),
  task("integration_calcom", "Cal.com", "#C9956A", "n8n"),
  task("integration_stripe", "Stripe", "#0570DE", "n8n"),
  task("integration_gitlab", "GitLab", "#FC6D26", "n8n"),
  task("integration_sentry", "Sentry", "#FF45A8", "n8n"),
  task("integration_linear", "Linear", "#5E6AD2", "n8n"),
  task("integration_netlify", "Netlify", "#32E6E2", "n8n"),
  task("integration_cloudflare", "Cloudflare", "#FACC15", "n8n"),
];

export const TOGGLEABLE_INTEGRATION_IDS = [
  "integration_resend", "integration_n8n", "integration_vercel", "integration_github",
  "integration_notion", "integration_calcom", "integration_stripe", "integration_gitlab",
  "integration_sentry", "integration_linear", "integration_netlify", "integration_cloudflare",
];

/** Pills next to the character besides VS Code — integrations and agents together. */
export const MAX_ACTIVE_PILLS = 4;

/**
 * Agent pill colours: the integration hues (Vercel violet, Resend green, n8n
 * orange, GitHub coral, Cal.com tan) plus the project colours, so a row of
 * pills reads as one family on the dark island.
 */
export const AGENT_PALETTE = [
  "#7C5CFF", "#38BDF8", "#2EC4A0", "#22C55E", "#EAB308",
  "#F29B38", "#F4505E", "#EC4899", "#A78BFA", "#C9956A",
] as const;

const HEX_COLOR = /^#[0-9A-Fa-f]{6}$/;
export const isHexColor = (c: string | null | undefined): c is string => !!c && HEX_COLOR.test(c);

/** A stable palette colour for an id (FNV-1a), so a pill keeps its colour. */
export function paletteColor(id: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    hash ^= id.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return AGENT_PALETTE[(hash >>> 0) % AGENT_PALETTE.length];
}

/** The chosen colour when valid, else the one derived from the id. */
export function agentColor(id: string, chosen?: string | null): string {
  return isHexColor(chosen) ? chosen.toUpperCase() : paletteColor(id);
}

export type AgentPillKind = "local" | "roadeep";
const PILL_PREFIX = "agent:";

/** settings.activeIntegrations id of an agent pill. */
export const agentPillId = (kind: AgentPillKind, id: string) => `${PILL_PREFIX}${kind}:${id}`;

export function parseAgentPill(pillId: string): { kind: AgentPillKind; id: string } | null {
  const m = /^agent:(local|roadeep):([A-Za-z0-9_-]{1,64})$/.exec(pillId);
  return m ? { kind: m[1] as AgentPillKind, id: m[2] } : null;
}

/** Every built-in local agent id starts with this (Rust: default_agents.json). */
export const DEFAULT_AGENT_PREFIX = "default-";
export const isDefaultAgent = (id: string) => id.startsWith(DEFAULT_AGENT_PREFIX);

/** Built-in agents on a new install's island, in pill order (Rust: default_agents::ISLAND). */
export const DEFAULT_ISLAND_AGENTS = [
  "default-letter-writer", "default-translator", "default-day-planner", "default-iranian-chef",
] as const;

/** The settings.chatAgent value an agent pill selects. */
export function chatRefOfPill(kind: AgentPillKind, id: string): string {
  return kind === "local" ? `local:${id}` : id;
}

/** What an integration poller last reported. */
export interface IntegrationInfo {
  data: Record<string, unknown>;
  error: string | null;
  loaded: boolean;
  configured: boolean;
}

export type AdminVoiceMode="manual"|"always";
export interface Settings {
  adminVoiceMode: AdminVoiceMode;
  characterAppearance: CharacterAppearance;
  observeCodex: boolean;
  retainCodingHistory: boolean;
  soundEnabled: boolean;
  soundVolume: number;
  autoCloseInterval: number;
  absenceInterval: number;
  activeIntegrations: string[];
  /**
   * Services the user added from the market (pill ids, native or catalog), in
   * the order they were added. Only these are listed under "My services".
   */
  addedIntegrations: string[];
  screen: "primary" | "cursor";
  autostart: boolean;
  hooksInstalled: boolean;
  /** Roadeep model id for new chat threads; "" = the server's default. */
  model: string;
  /** UI language; Rust persists it with the same "fa" default. */
  language: Language;
  /** Island chat agent: a Roadeep agent id, "local:<id>", or null for none. */
  chatAgent: string | null;
  /** Web search and reasoning exclude each other; deep research implies web search. */
  chatWebSearch: boolean;
  chatReasoning: boolean;
  chatReasoningEffort: ReasoningEffort;
  chatDeepResearch: boolean;
  /**
   * Offer the enabled MCP servers' tools in the island chat; the chat's tools
   * chip turns them off for one conversation.
   */
  chatTools: boolean;
  /** Pill colours picked for Roadeep agents (id → "#RRGGBB"). */
  agentColors: Record<string, string>;
  /** Global shortcut that opens the island chat, e.g. "Ctrl+Alt+Space"; "" = off. */
  shortcut: string;
  /** Look for a new version once a day (builds with the updater only). */
  autoUpdateCheck: boolean;
  /** Where the island is docked. Rust owns it (drag, dockSet); read-only here. */
  dock: DockSettings;
  /** Planner focus timer, whole minutes, within PLANNER_BOUNDS (Rust clamps too). */
  focusMinutes: number;
  breakMinutes: number;
  longBreakMinutes: number;
  /** Focus rounds before a long break instead of a short one. */
  roundsBeforeLongBreak: number;
  /** Sound when a focus or break phase ends / when a reminder fires. */
  focusSound: boolean;
  reminderSound: boolean;
  /** Habit nudges («یه لیوان آب؟») from the planner scheduler. */
  habitNudges: boolean;
  /** How restless the character's idle eyes are (character/engine.ts). */
  eyeMotion: EyeMotion;
  /** Small particle burst when a task or a focus round completes. */
  celebrations: boolean;
}

export type { EyeMotion };
export const EYE_MOTIONS: EyeMotion[] = ["normal", "calm", "still"];

/** Limits of the planner durations; Rust clamps with the same numbers (settings.rs). */
export const PLANNER_BOUNDS = {
  focusMinutes: { min: 5, max: 120 },
  breakMinutes: { min: 1, max: 30 },
  longBreakMinutes: { min: 5, max: 60 },
  roundsBeforeLongBreak: { min: 2, max: 8 },
} as const;

export interface DockSettings {
  edge: "top" | "left" | "right";
  /** Centre along the edge, 0…1 of the display's work area. */
  pos: number;
  /** The display the island was dragged to; "" = the one `screen` picks. */
  monitor: string;
}

export const DEFAULT_DOCK: DockSettings = { edge: "top", pos: 0.5, monitor: "" };

export type ReasoningEffort = "low" | "medium" | "high";
export const REASONING_EFFORTS: ReasoningEffort[] = ["low", "medium", "high"];

/** Where the Roadeep session stands in this window; null = not known yet. */
export interface RoadeepState {
  signedIn: boolean | null;
  user: RoadeepUser | null;
  /** The last sign-out was an expiry, not a click. */
  expired: boolean;
  agents: RoadeepAgent[];
  localAgents: LocalAgent[];
  /** Ids in `agents` that only this account can use (the catalogue's private group). */
  exclusiveIds: string[];
  /** Both lists have been read at least once since sign-in. */
  agentsLoaded: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  adminVoiceMode: "manual",
  soundEnabled: true,
  observeCodex: false,
  retainCodingHistory: false,
  characterAppearance: { ...DEFAULT_APPEARANCE },
  soundVolume: 0.12,
  autoCloseInterval: 15,
  /** Seconds before the untouched island becomes a line (core/idle.ts); 0 = never. */
  absenceInterval: 300,
  // Built-in agents, not services: a service is listed once the user adds it.
  activeIntegrations: DEFAULT_ISLAND_AGENTS.map((id) => agentPillId("local", id)),
  addedIntegrations: [],
  screen: "primary",
  autostart: false,
  hooksInstalled: false,
  model: "",
  language: DEFAULT_LANGUAGE,
  chatAgent: null,
  chatWebSearch: false,
  chatReasoning: false,
  chatReasoningEffort: "low",
  chatDeepResearch: false,
  chatTools: true,
  agentColors: {},
  shortcut: "Ctrl+Alt+KeyR",
  autoUpdateCheck: true,
  dock: { ...DEFAULT_DOCK },
  focusMinutes: 25,
  breakMinutes: 5,
  longBreakMinutes: 15,
  roundsBeforeLongBreak: 4,
  focusSound: true,
  reminderSound: true,
  habitNudges: true,
  eyeMotion: "normal",
  celebrations: true,
};

type Listener = () => void;

class AppState {
  mode: IslandMode = "hidden";
  view: IslandViewName = "overview";

  tasks: AgentTask[] = [];
  focusId: string | null = null;

  stateOverride: BotStateName | null = null;
  activityBotState: BotStateName | null = null;

  /** Cursor in logical screen pixels, origin top-left (like AppState.mousePosition). */
  mouse = { x: 0, y: 0 };
  /** Cursor relative to the island's top-left corner. */
  mouseInIsland = { x: 0, y: 0 };

  isPinned = false;
  paused = false;
  /** The chat is maximised (a large island). For this session only. */
  maximized = false;

  uploadProgress = 0;
  uploadDuration = 2.4;
  fileDragOver = false;

  promptContext: PromptContext | null = null;
  droppedFile: { name: string; path: string } | null = null;
  noteMessage: string | null = null;
  searchResult: SearchResult | null = null;
  chatHistory: ChatMessage[] = [];
  pendingApproval: ApprovalInfo | null = null;

  integrations: Record<string, IntegrationInfo> = {};

  roadeep: RoadeepState = {
    signedIn: null, user: null, expired: false, agents: [], localAgents: [], exclusiveIds: [],
    agentsLoaded: false,
  };

  lastActivity = performance.now();

  settings: Settings = { ...DEFAULT_SETTINGS };

  private listeners = new Set<Listener>();

  subscribe(fn: Listener): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** Marks the UI dirty; the island re-renders on the next frame. */
  notify() {
    for (const fn of this.listeners) fn();
  }

  get focusTask(): AgentTask | null {
    return this.tasks.find((t) => t.id === this.focusId) ?? this.tasks[0] ?? null;
  }

  get effectiveState(): BotStateName {
    return this.stateOverride ?? (this.view === "activity" ? this.activityBotState : null) ?? this.focusTask?.state ?? "idle";
  }

  get otherTasks(): AgentTask[] {
    return this.tasks.filter((t) => t.id !== this.focusId);
  }

  setFocus(id: string) {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) return;
    this.focusId = id;
    t.pillBadge = null;
    this.notify();
  }

  updateTask(id: string, state: BotStateName) {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) return;
    t.state = state;
    this.notify();
  }

  appendStep(id: string, step: string) {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) return;
    t.steps.push(step);
    if (t.steps.length > 20) t.steps.shift();
    t.stepIndex = t.steps.length - 1;
    this.notify();
  }

  setPillBadge(id: string, badge: PillBadge | null) {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) return;
    t.pillBadge = badge;
    this.notify();
  }

  /** Name and colour of an agent pill from the lists read so far. */
  private agentLook(kind: AgentPillKind, id: string): { name: string; color: string } {
    if (kind === "local") {
      const a = this.roadeep.localAgents.find((x) => x.id === id);
      return { name: a?.name ?? "…", color: agentColor(id, a?.color) };
    }
    const a = this.roadeep.agents.find((x) => x.id === id);
    return { name: a?.title ?? "…", color: agentColor(id, this.settings.agentColors?.[id]) };
  }

  /**
   * loadIntegrationTasks() — VS Code always on, the rest opt-in (max 4 pills,
   * integrations and agents together). Catalog services, then agent pills, follow
   * the native integrations in the order they were switched on; their name and
   * colour are refreshed here, so call this again once the agent lists or the
   * catalog arrive.
   */
  loadIntegrationTasks() {
    const active = this.settings.activeIntegrations;
    for (const proto of INTEGRATION_AGENTS) {
      const shouldLoad = proto.id === "integration_claude" || active.includes(proto.id);
      const idx = this.tasks.findIndex((t) => t.id === proto.id);
      if (shouldLoad && idx < 0) this.tasks.push({ ...proto, steps: [] });
      if (!shouldLoad && idx >= 0) this.tasks.splice(idx, 1);
    }
    // Catalog services take their name and colour from the catalog; one it
    // hasn't loaded yet gets its pill once it has (loadIntegrationTasks again).
    const catalogPills = active.filter((id) => !INTEGRATION_AGENTS.some((p) => p.id === id) && catalogEntryOf(id));
    this.tasks = this.tasks.filter((t) => !t.isCatalog || catalogPills.includes(t.id));
    for (const pillId of catalogPills) {
      const entry = catalogEntryOf(pillId)!;
      const existing = this.tasks.find((t) => t.id === pillId);
      if (existing) {
        existing.name = entry.name;
        existing.color = entry.color;
      } else {
        this.tasks.push({ ...task(pillId, entry.name, entry.color, "n8n"), isCatalog: true });
      }
    }
    const agentPills = active.filter((id) => parseAgentPill(id));
    this.tasks = this.tasks.filter((t) => !t.agentRef || agentPills.includes(t.id));
    for (const pillId of agentPills) {
      const pill = parseAgentPill(pillId)!;
      const look = this.agentLook(pill.kind, pill.id);
      const existing = this.tasks.find((t) => t.id === pillId);
      if (existing) {
        existing.name = look.name;
        existing.color = look.color;
      } else {
        this.tasks.push({
          id: pillId, name: look.name, color: look.color, state: "idle", stepIndex: 0, steps: [],
          source: "agent", isIntegration: true, agentRef: chatRefOfPill(pill.kind, pill.id),
        });
      }
    }
    if (this.focusId && !this.tasks.some((t) => t.id === this.focusId)) this.focusId = "integration_claude";
    // Keep the declared order so pills never shuffle.
    const order = [...INTEGRATION_AGENTS.map((t) => t.id), ...catalogPills, ...agentPills];
    this.tasks.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id));
    if (!this.focusId) this.focusId = "integration_claude";
    this.notify();
  }

  toggleIntegration(id: string) {
    if (id === "integration_claude") return;
    const active = this.settings.activeIntegrations;
    if (active.includes(id)) {
      this.settings.activeIntegrations = active.filter((x) => x !== id);
      if (this.focusId === id) this.focusId = "integration_claude";
    } else {
      if (active.length >= MAX_ACTIVE_PILLS) return;
      this.settings.activeIntegrations = [...active, id];
    }
    this.loadIntegrationTasks();
  }

  defaultView(): IslandViewName {
    return this.tasks.length === 0 ? "empty" : "overview";
  }
}

export const State = new AppState();
