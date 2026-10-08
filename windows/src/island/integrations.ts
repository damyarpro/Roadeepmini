// Integration events → island state. Port of the `handle…` methods in the Swift
// pollers: a genuinely new item flips the pill to finished/error, badges it when
// the pill isn't focused, plays a sound, and clears itself after 60 s.

import { onEvent, Bridge, IS_TAURI, type IntegrationUpdate } from "../core/bridge";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import { catalogEntryOf, loadCatalog } from "../core/catalog";
import type { Island } from "./island";

/** Which Credential Manager key backs each pill. */
const KEY_FOR: Record<string, string> = {
  integration_stripe: "stripe-api-key",
  integration_github: "github-token",
  integration_vercel: "vercel-token",
  integration_n8n: "n8n-api-key",
  integration_resend: "resend-api-key",
  integration_notion: "notion-api-key",
  integration_calcom: "calcom-api-key",
  integration_gitlab: "gitlab-token",
  integration_sentry: "sentry-token",
  integration_linear: "linear-api-key",
  integration_netlify: "netlify-token",
  integration_cloudflare: "cloudflare-token",
};

const clearTimers = new Map<string, number>();

export function registerIntegrationHandlers(island: Island) {
  void onEvent<IntegrationUpdate>("integration", (update) => handle(island, update));
  void refreshConfigured();
  // Catalog pills need the catalog's names and colours; they appear once it is read.
  void loadCatalog().then((list) => {
    if (list.length === 0) return;
    State.loadIntegrationTasks();
    void refreshConfigured();
  });
}

/** A catalog service is configured once every required field is stored. */
async function catalogConfigured(pillId: string): Promise<boolean | null> {
  const entry = catalogEntryOf(pillId);
  if (!entry) return null;
  for (const field of entry.fields) {
    if (!field.optional && !((await Bridge.secretPresent(field.key)) ?? false)) return false;
  }
  return true;
}

/** Asks Rust which keys exist so the idle cards can say so. */
export async function refreshConfigured() {
  for (const [id, key] of Object.entries(KEY_FOR)) {
    const present = (await Bridge.secretPresent(key)) ?? false;
    const info = State.integrations[id] ?? { data: {}, error: null, loaded: false, configured: false };
    State.integrations[id] = { ...info, configured: present };
  }
  // Only the catalog services that have a pill: the catalog can be long.
  for (const task of State.tasks.filter((x) => x.isCatalog)) {
    const configured = await catalogConfigured(task.id);
    if (configured == null) continue;
    const info = State.integrations[task.id] ?? { data: {}, error: null, loaded: false, configured: false };
    State.integrations[task.id] = { ...info, configured };
  }
  let hooks = State.settings.hooksInstalled;
  try {
    for (const status of await Bridge.codingHooksStatus()) {
      const id = `integration_${status.provider}`;
      const info = State.integrations[id] ?? {data:{},error:null,loaded:false,configured:false};
      State.integrations[id] = {...info, configured:status.installed};
      if (status.provider === "claude") hooks = status.installed;
    }
  } catch { if (IS_TAURI) void Bridge.log("coding hook connection status unavailable"); }
  const claude = State.integrations.integration_claude ?? {
    data: {}, error: null, loaded: false, configured: false,
  };
  State.integrations.integration_claude = { ...claude, configured: hooks };
  State.notify();
}

function handle(island: Island, update: IntegrationUpdate) {
  if (State.paused) return;

  const previous = State.integrations[update.id];
  State.integrations[update.id] = {
    data: update.error ? (previous?.data ?? {}) : update.data,
    error: update.error,
    loaded: update.error ? (previous?.loaded ?? false) : true,
    configured: previous?.configured ?? true,
  };

  const event = update.event;
  if (event) {
    const task = State.tasks.find((t) => t.id === update.id);
    if (task) {
      task.state = event.success ? "finished" : "error";
      task.steps = event.detail ? [event.label, event.detail] : [event.label];
      task.stepIndex = task.steps.length - 1;
      if (State.focusId !== update.id) {
        task.pillBadge = event.success ? "finished" : "error";
      }
      Sound.play(event.success ? "finish" : "error");
      // Same as the Swift pollers: show the compact island so the badge is seen,
      // but never steal the screen for a successful deploy.
      island.reveal();

      const existing = clearTimers.get(update.id);
      if (existing != null) window.clearTimeout(existing);
      clearTimers.set(
        update.id,
        window.setTimeout(() => {
          clearTimers.delete(update.id);
          const t = State.tasks.find((x) => x.id === update.id);
          if (!t || (t.state !== "finished" && t.state !== "error")) return;
          t.state = "idle";
          t.steps = [];
          t.stepIndex = 0;
          t.pillBadge = null;
          State.notify();
        }, 60_000),
      );
    }
  }

  State.notify();
}
