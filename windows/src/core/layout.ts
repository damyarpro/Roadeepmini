// Island geometry — ported from IslandTypes.swift + IslandWindowController.islandSize
// + IslandRootView.botPosition. All values are logical pixels, identical to the
// macOS app's points.

export type IslandMode = "hidden" | "compact" | "expanded";

export type IslandViewName =
  | "activity"
  | "overview"
  | "empty"
  | "approval"
  | "question"
  | "error"
  | "finished"
  | "confused"
  | "upload"
  | "uploading"
  | "choose"
  | "mail"
  | "prompt"
  | "searching"
  | "result"
  | "note"
  | "settings"
  | "greeting"
  // Planner (src/views/palette.ts, src/views/planner): the «/» menu, its views and alerts.
  | "menu"
  | "tasks"
  | "notes"
  | "reminders"
  | "habits"
  | "today"
  | "week"
  | "focus"
  | "plannerFire"
  // Live voice: a change waiting for the user's approval (src/local/live-ui.ts).
  | "voiceApproval";

export type BotStateName =
  | "idle"
  | "working"
  | "thinking"
  | "searching"
  | "approval"
  | "question"
  | "error"
  | "finished"
  | "ratelimit"
  | "sleeping"
  | "dizzy";

export type BotEmoteName = "love" | "surprised" | "proud" | "wink" | "yawn" | "happy" | "annoyed";

export type AgentLayoutMode = "none" | "grid" | "pills" | "column";

export interface ViewLayout {
  height: number;
  botX: number;
  botY: number | null; // null = auto-centred
  botDiameter: number;
  agentMode: AgentLayoutMode;
}

// The window is at least 720×480; Rust sizes it per display and dock edge to
// hold the largest island there (src-tauri/src/dock.rs) and the island is drawn
// inside it against its dock edge (src/island/dock.ts). Everything around the
// island is click-through (Rust tests the pushed island rect). Keep in step
// with PANEL_W / PANEL_H in src-tauri/src/island.rs.
export const PANEL_W = 720;
export const PANEL_H = 480;

// No notch on a PC: these are the hidden/compact sizes from docs/SPEC.md.
export const NOTCH_W = 184;
export const NOTCH_H = 32;
export const VOICE_COMPACT_H = 48;
export const HEADER_EXTRA_H = 14;
export const COMPACT_W = 288; // NOTCH_W + 104
export const EXPANDED_W = 640;

export const ROUNDED_CORNER = 14; // hidden / compact
export const EXPANDED_CORNER = 22;

/**
 * Hover strip that wakes the hidden island and carries its line: the line
 * (COMPACT_W plus a 5 px shoulder at each end, style.css) and a little slack.
 * Same as STRIP_W / STRIP_H in src-tauri/src/island.rs.
 */
export const WAKE_STRIP_W = 304;
export const WAKE_STRIP_H = 8;

export const VIEW_LAYOUTS: Record<IslandViewName, ViewLayout> = {
  activity: { height: 430, botX: 38, botY: 73, botDiameter: 28, agentMode: "none" },
  overview: { height: 160, botX: 68, botY: null, botDiameter: 58, agentMode: "pills" },
  empty: { height: 160, botX: 70, botY: null, botDiameter: 62, agentMode: "none" },
  approval: { height: 160, botX: 62, botY: null, botDiameter: 56, agentMode: "column" },
  question: { height: 160, botX: 62, botY: null, botDiameter: 56, agentMode: "column" },
  error: { height: 160, botX: 62, botY: null, botDiameter: 58, agentMode: "column" },
  finished: { height: 160, botX: 62, botY: null, botDiameter: 58, agentMode: "column" },
  confused: { height: 160, botX: 76, botY: null, botDiameter: 66, agentMode: "column" },
  upload: { height: 176, botX: 140, botY: 104, botDiameter: 62, agentMode: "column" },
  // botY 103 = bar top (42 + 58) + 3, so the dot really rides the bar. The Swift
  // layout says 118 while its own comment says 103; the comment matches the spec.
  uploading: { height: 176, botX: 46, botY: 103, botDiameter: 20, agentMode: "none" },
  choose: { height: 176, botX: 60, botY: 101, botDiameter: 52, agentMode: "column" },
  mail: { height: 240, botX: 56, botY: null, botDiameter: 46, agentMode: "column" },
  prompt: { height: 160, botX: 52, botY: null, botDiameter: 44, agentMode: "column" },
  searching: { height: 160, botX: 52, botY: null, botDiameter: 44, agentMode: "column" },
  result: { height: 160, botX: 52, botY: null, botDiameter: 44, agentMode: "column" },
  note: { height: 160, botX: 60, botY: null, botDiameter: 50, agentMode: "column" },
  settings: { height: 160, botX: 54, botY: null, botDiameter: 46, agentMode: "none" },
  greeting: { height: 150, botX: 320, botY: 90, botDiameter: 0, agentMode: "none" },
  // Planner lists: the character stands by the title row (botY), the list
  // scrolls below. Taller than CHAT_MAX_H on a small display, they shrink to it
  // (islandSize) and scroll.
  menu: { height: 340, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  tasks: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  notes: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  reminders: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  habits: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  today: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  week: { height: 320, botX: 54, botY: 92, botDiameter: 46, agentMode: "none" },
  focus: { height: 236, botX: 54, botY: null, botDiameter: 46, agentMode: "none" },
  plannerFire: { height: 160, botX: 62, botY: null, botDiameter: 56, agentMode: "none" },
  voiceApproval: { height: 236, botX: 62, botY: null, botDiameter: 56, agentMode: "none" },
};

// The upload views above are only the fallback geometry. Once a file is actually
// dropped the whole sequence — the character included — is drawn by src/upload, which
// owns its own constants (USC) straight from UploadSequenceEngine.swift.

/**
 * Shortest and tallest the chat gets. The tallest follows the display: up to
 * 100 px above the bottom of its work area (Rust computes it per dock, see
 * dock.rs `layout`); 440 until Rust has said.
 */
export const CHAT_MIN_H = 240;
export let CHAT_MAX_H = 440;

export function setChatMaxH(value: number) {
  if (Number.isFinite(value)) CHAT_MAX_H = Math.max(CHAT_MIN_H, Math.round(value));
}

/**
 * Chat view height. `wanted` is what the chat measured its content to need
 * (the history list asks for the maximum); without a measurement it grows with
 * the message count like IslandContainer.chatPromptHeight.
 */
export function chatPromptHeight(messageCount: number, wanted: number | null = null): number {
  if (wanted != null) return Math.round(Math.min(CHAT_MAX_H, Math.max(CHAT_MIN_H, wanted)));
  return Math.min(300, CHAT_MIN_H + messageCount * 40);
}

export function islandSize(
  mode: IslandMode,
  view: IslandViewName,
  chatCount = 0,
  chatWanted: number | null = null,
  voiceActive = false,
): { w: number; h: number } {
  switch (mode) {
    case "hidden":
      // No notch to hide inside on a PC: the island retracts to zero height and
      // slides into the top edge of the screen instead of sitting there as a bar.
      return { w: NOTCH_W, h: 0 };
    case "compact":
      return { w: COMPACT_W, h: voiceActive ? VOICE_COMPACT_H : NOTCH_H };
    case "expanded": {
      // No fixed view outgrows the chat's tallest (at least CHAT_MIN_H, so only
      // the taller planner views can be held back on a small display).
      const h = view === "prompt" ? chatPromptHeight(chatCount, chatWanted) : Math.min(VIEW_LAYOUTS[view].height + HEADER_EXTRA_H, CHAT_MAX_H);
      return { w: EXPANDED_W, h };
    }
  }
}

export interface BotPlacement {
  cx: number;
  cy: number;
  diameter: number;
  opacity: number;
}

/** IslandRootView.botPosition — cy is measured from the island's top edge. */
export function botPosition(
  mode: IslandMode,
  view: IslandViewName,
  islandH: number,
  uploadProgress = 0,
): BotPlacement {
  switch (mode) {
    case "hidden":
      return { cx: 46, cy: 16, diameter: 6, opacity: 0 };
    case "compact":
      return { cx: 40, cy: 16, diameter: 20, opacity: 1 };
    case "expanded": {
      const layout = VIEW_LAYOUTS[view];
      if (view === "uploading") {
        return {
          cx: 36 + uploadProgress * 526,
          cy: (layout.botY ?? 103) + HEADER_EXTRA_H,
          diameter: layout.botDiameter,
          opacity: 1,
        };
      }
      if (layout.botY != null) {
        return { cx: layout.botX, cy: layout.botY + HEADER_EXTRA_H, diameter: layout.botDiameter, opacity: 1 };
      }
      // Centre of the fixed 84 pt card (8 pt top inset + 34 pt header → content at y = 42)
      const headerBottom = 42 + HEADER_EXTRA_H;
      const cardH = 84;
      const cy = headerBottom + (islandH - headerBottom - cardH) / 2 + cardH / 2;
      return { cx: layout.botX, cy, diameter: layout.botDiameter, opacity: 1 };
    }
  }
}

export function botGlowColor(s: BotStateName): string {
  switch (s) {
    case "working":
      return "#3B9EFF";
    case "thinking":
      return "#A78BFA";
    case "searching":
      return "#6366F1";
    case "approval":
      return "#F5A524";
    case "error":
      return "#F4505E";
    case "finished":
      return "#34D399";
    case "ratelimit":
      return "#F59E0B";
    default:
      return "#FFFFFF";
  }
}

export function botGlowOpacity(s: BotStateName): number {
  switch (s) {
    case "idle":
    case "sleeping":
      return 0.15;
    case "dizzy":
      return 0;
    default:
      return 0.65;
  }
}

// Project colours (IslandConst.projectColors)
const PROJECT_COLORS: Record<string, string> = {
  korus: "#FF5A4E",
  "sbe hub": "#2EC4A0",
  "morning ai brief": "#F29B38",
  "publication ig": "#7C5CFF",
  "ig post": "#7C5CFF",
  "louisraille.fr": "#38BDF8",
  louisraille: "#38BDF8",
  "notch buddy": "#EC4899",
  "notch-buddy": "#EC4899",
  notchbuddy: "#EC4899",
};

const FALLBACK_COLORS = ["#22C55E", "#EAB308", "#60A5FA", "#E879F9"];

export function colorForProject(name: string): string {
  const key = name.toLowerCase().trim();
  const exact = PROJECT_COLORS[key];
  if (exact) return exact;
  for (const [k, c] of Object.entries(PROJECT_COLORS)) {
    if (key.startsWith(k) || key.includes(k)) return c;
  }
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) | 0;
  return FALLBACK_COLORS[Math.abs(hash) % FALLBACK_COLORS.length];
}

// Card wash colours (CardBackground.washColor)
export type Wash = "red" | "green" | "pink" | "amber" | "cyan" | "indigo" | "soft" | null;

export function washRGBA(wash: Wash): string {
  switch (wash) {
    case "red":
      return "rgba(244,80,94,0.55)";
    case "green":
      return "rgba(52,211,153,0.5)";
    case "pink":
      return "rgba(244,114,182,0.55)";
    case "amber":
      return "rgba(245,165,36,0.42)";
    case "cyan":
      return "rgba(34,211,238,0.38)";
    case "indigo":
      return "rgba(99,102,241,0.5)";
    case "soft":
      return "rgba(255,255,255,0.08)";
    default:
      return "rgba(0,0,0,0)";
  }
}
