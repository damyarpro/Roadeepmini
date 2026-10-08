// The island's view of the Roadeep session and of the agents its chat can use.
// Sign-ins and sign-outs come from the settings window (or an expiry) through
// the "roadeep-session" event; the session is also read once at boot. Local
// agents edited in the settings window arrive as "local-agents-changed".

import {
  Bridge, isRoadeepError, onEvent, onRoadeepSession, LOCAL_AGENT_PREFIX, LOCAL_AGENTS_EVENT,
  type RoadeepSession,
} from "../core/bridge";
import { State, parseAgentPill, type Settings } from "../core/state";

function apply(session: RoadeepSession) {
  const r = State.roadeep;
  const wasSignedIn = r.signedIn;
  r.signedIn = session.signedIn;
  r.user = session.user;
  r.expired = !session.signedIn && session.reason === "expired";
  if (!session.signedIn) {
    r.agents = [];
    r.exclusiveIds = [];
    r.agentsLoaded = false;
    // Rust has already dropped the thread; the log on screen goes with it.
    if (wasSignedIn && State.chatHistory.length) State.chatHistory = [];
  }
  State.notify();
  void refreshAgents();
}

let refreshing: Promise<void> | null = null;

/**
 * Re-reads both agent lists. Local agents can change in the settings window at
 * any time, so the chat calls this whenever it opens. A failed read keeps the
 * last good list.
 */
export function refreshAgents(): Promise<void> {
  if (refreshing) return refreshing;
  refreshing = (async () => {
    const r = State.roadeep;
    let localOk = true;
    let roadeepOk = !r.signedIn;
    try {
      r.localAgents = await Bridge.localAgentsList();
    } catch (err) {
      localOk = false;
      void Bridge.log(`chat: local agents unavailable: ${isRoadeepError(err) ? err.code : String(err)}`);
    }
    if (r.signedIn) {
      try {
        const catalog = await Bridge.roadeepAgentCatalog();
        r.exclusiveIds = catalog.exclusive.map((a) => a.id);
        r.agents = [...catalog.exclusive, ...catalog.public];
        roadeepOk = true;
      } catch (err) {
        void Bridge.log(`chat: roadeep agents unavailable: ${isRoadeepError(err) ? err.code : String(err)}`);
      }
    }
    if (localOk && roadeepOk) {
      r.agentsLoaded = true;
      dropStaleAgents(localOk, r.signedIn === true && roadeepOk);
    } else if (localOk) {
      dropStaleAgents(true, false);
    }
    // Pills carry the agents' names and colours.
    State.loadIntegrationTasks();
  })().finally(() => {
    refreshing = null;
  });
  return refreshing;
}

/**
 * A chosen agent or a pill whose agent was deleted (or is no longer offered)
 * goes away. Only lists that were really read are trusted for this.
 */
function dropStaleAgents(localKnown: boolean, roadeepKnown: boolean) {
  const r = State.roadeep;
  const localExists = (id: string) => r.localAgents.some((a) => a.id === id);
  const roadeepExists = (id: string) => r.agents.some((a) => a.id === id);
  let changed = false;

  const chosen = State.settings.chatAgent;
  if (chosen) {
    const isLocal = chosen.startsWith(LOCAL_AGENT_PREFIX);
    const gone = isLocal
      ? localKnown && !localExists(chosen.slice(LOCAL_AGENT_PREFIX.length))
      : roadeepKnown && !roadeepExists(chosen);
    if (gone) {
      void Bridge.log("chat: the chosen agent is gone, back to none");
      State.settings.chatAgent = null;
      changed = true;
    }
  }

  const pills = State.settings.activeIntegrations.filter((id) => {
    const pill = parseAgentPill(id);
    if (!pill) return true;
    if (pill.kind === "local") return !localKnown || localExists(pill.id);
    return !roadeepKnown || roadeepExists(pill.id);
  });
  if (pills.length !== State.settings.activeIntegrations.length) {
    void Bridge.log(`island: dropped ${State.settings.activeIntegrations.length - pills.length} pill(s) of deleted agents`);
    State.settings.activeIntegrations = pills;
    changed = true;
  }
  if (changed) void Bridge.saveSettings(State.settings);
}

/** Wires the session and agent events and reads the current session once. */
export async function initRoadeepSession() {
  await onRoadeepSession(apply);
  await onEvent<null>(LOCAL_AGENTS_EVENT, () => void refreshAgents());
  // A pill switched on in the settings window for an agent this window has not
  // read yet would show "…" until the next refresh.
  await onEvent<Settings>("settings-changed", (s) => {
    const r = State.roadeep;
    const unknown = (s.activeIntegrations ?? []).some((id) => {
      const pill = parseAgentPill(id);
      if (!pill) return false;
      return pill.kind === "local"
        ? !r.localAgents.some((a) => a.id === pill.id)
        : !r.agents.some((a) => a.id === pill.id);
    });
    if (unknown) void refreshAgents();
  });
  try {
    apply(await Bridge.roadeepSession());
  } catch (err) {
    // Outside the app (NOT_IN_APP) or a broken bridge: treat as signed out so
    // the chat shows the sign-in prompt instead of a dead input.
    void Bridge.log(`roadeep: session unknown at boot: ${isRoadeepError(err) ? err.code : String(err)}`);
    apply({ signedIn: false, user: null, reason: null });
  }
}
