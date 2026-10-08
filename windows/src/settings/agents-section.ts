// Settings window — the "Agents" section: the local agents
// (built with agent-wizard.ts, stored in agents.json) and Roadeep's agents,
// split into this account's exclusive ones and the public ones. Every row has
// the same pill switch as the integrations (pills.ts).

import {
  Bridge, isRoadeepError, localAgentRef, onEvent, LOCAL_AGENTS_EVENT, LOCAL_AGENT_LIMITS,
  type LocalAgent, type RoadeepAgent,
} from "../core/bridge";
import { AGENT_PALETTE, agentColor, agentPillId, isDefaultAgent, isHexColor, type Settings } from "../core/state";
import { isolate, t } from "../core/i18n";
import { h, clear } from "../views/dom";
import { roadeepErrorText } from "../views/errors";
import { onRoadeepData, openRoadeepSite, roadeepData } from "./roadeep-section";
import { openAgentWizard } from "./agent-wizard";
import { pillSwitch, setPill } from "./pills";
import { icon, linkButton, helpDisclosure } from "./ui";

export interface AgentsHost {
  settings: () => Settings;
  save: () => Promise<void>;
}

let host: AgentsHost;

const ag = {
  local: [] as LocalAgent[],
  localError: null as string | null,
  /** Local agent whose delete is being confirmed. */
  confirmingDelete: null as string | null,
  /** Roadeep agent whose colour picker is open. */
  colorOpen: null as string | null,
  /** One-off line after a save ("Saved …"). */
  flash: null as string | null,
};

const slot = h("div", { class: "slot" });

function log(message: string) {
  void Bridge.log(`settings: ${message}`);
}

async function loadLocal() {
  try {
    ag.local = await Bridge.localAgentsList();
    ag.localError = null;
  } catch (err) {
    // Outside the app there is no store to read; that is not worth a red box.
    ag.localError = isRoadeepError(err) && err.code === "NOT_IN_APP" ? null : roadeepErrorText(err);
    log(`local agents failed ${isRoadeepError(err) ? err.code : String(err)}`);
  }
  draw();
}

/** Call once before the first render. */
export function initAgentsSection(h0: AgentsHost) {
  host = h0;
  onRoadeepData(draw);
  void onEvent<null>(LOCAL_AGENTS_EVENT, () => void loadLocal());
  void loadLocal();
}

/** The agent groups; the same element across redraws. */
export function agentGroups(): HTMLElement {
  draw();
  return slot;
}

function openWizard(editing?: LocalAgent) {
  ag.confirmingDelete = null;
  ag.flash = null;
  openAgentWizard({
    settings: host.settings,
    saved: (agent) => {
      ag.flash = t(editing ? "agents.savedEdit" : "agents.savedNew", { name: isolate(agent.name) });
      void loadLocal();
    },
  }, editing);
}

function draw() {
  if (!host) return;
  clear(slot);
  slot.append(localGroup(), roadeepGroup());
}

// ── My agents ─────────────────────────────────────────────────────────────────

function swatchDot(color: string): HTMLElement {
  return h("i", { class: "agent-swatch", style: `--c:${color}`, "aria-hidden": "true" });
}

function localGroup(): HTMLElement {
  const count = ag.local.length;
  const full = count >= LOCAL_AGENT_LIMITS.maxAgents;
  const build = h("button", { type: "button", class: "primary sm", onclick: () => openWizard() },
    icon("plus", 16), h("span", { text: t("agents.build") })) as HTMLButtonElement;
  build.disabled = full;
  const group = h("div", { class: "group card", role: "group", "aria-labelledby": "grp-local" },
    h("div", { class: "subhead" },
      h("h3", { id: "grp-local", text: t("agents.localTitle") }),
      h("span", { class: "hint", text: t("agents.limit", { n: count, max: LOCAL_AGENT_LIMITS.maxAgents }) }),
      h("div", { class: "spacer" }),
      // With no agents yet the button sits in the empty card instead.
      count > 0 ? build : null,
    ),
    helpDisclosure(t("agents.localHint"), `${t("settings.help")}: ${t("agents.localTitle")}`),
  );
  if (ag.flash) group.append(h("div", { class: "notice ok", role: "status", dir: "auto", text: ag.flash }));
  if (full) group.append(h("div", { class: "notice warn", text: t("rerr.LOCAL_AGENT_LIMIT") }));
  if (ag.localError) group.append(h("div", { class: "notice err", text: t("agents.loadFailed", { err: ag.localError }) }));

  if (count === 0) {
    group.append(h("div", { class: "empty-card" },
      h("div", { class: "empty-art", "aria-hidden": "true" },
        ...AGENT_PALETTE.slice(0, 4).map((c) => swatchDot(c))),
      h("div", { class: "agent-text" },
        h("div", { class: "agent-name", text: t("agents.emptyTitle") }),
        h("div", { class: "hint", text: t("agents.emptyHint") }),
      ),
      build,
    ));
    return group;
  }
  const list = h("ul", { class: "agent-list" });
  for (const a of ag.local) list.append(localRow(a));
  group.append(list);
  return group;
}

function modelName(id: string): string {
  const m = roadeepData().models?.find((x) => x.id === id);
  return m ? m.displayName || m.id : id;
}

function localRow(a: LocalAgent): HTMLElement {
  const color = agentColor(a.id, a.color);
  const meta: string[] = [a.model ? modelName(a.model) : t("agents.accountDefault")];
  if (isDefaultAgent(a.id)) meta.unshift(t("agents.builtInBadge"));
  if (a.webSearch) meta.push(t("agents.webBadge"));
  if (a.starterPrompts.length) meta.push(t("wizard.promptsCount", { n: a.starterPrompts.length }));
  const sw = pillSwitch(agentPillId("local", a.id), t("pills.showNamed", { name: a.name }));
  const item = h("li", { class: "agent-item" },
    swatchDot(color),
    h("div", { class: "agent-text" },
      h("div", { class: "agent-name", dir: "auto", text: a.name }),
      a.description ? h("div", { class: "hint", dir: "auto", text: a.description }) : null,
      // Each part isolated: a Latin model name must not flip the order.
      h("div", { class: "hint meta", text: meta.map(isolate).join(" · ") }),
    ),
  );
  const actions = h("div", { class: "agent-actions" });
  if (ag.confirmingDelete === a.id) {
    const yes = h("button", { type: "button", class: "danger sm", text: t("agents.delete") }) as HTMLButtonElement;
    yes.addEventListener("click", async () => {
      yes.disabled = true;
      try {
        await Bridge.localAgentDelete(a.id);
        ag.confirmingDelete = null;
        setPill(agentPillId("local", a.id), false);
        if (host.settings().chatAgent === localAgentRef(a.id)) {
          host.settings().chatAgent = null;
          void host.save();
        }
        log("deleted a local agent");
        await loadLocal();
      } catch (err) {
        log(`agent delete failed ${isRoadeepError(err) ? err.code : String(err)}`);
        yes.disabled = false;
        item.append(h("div", { class: "notice err", text: roadeepErrorText(err) }));
      }
    });
    actions.append(
      h("span", { class: "confirm", role: "alert", dir: "auto", text: t("agents.confirmDelete", { name: a.name }) }),
      yes,
      h("button", { type: "button", class: "sm", text: t("common.cancel"), "data-focus": "1", onclick: () => { ag.confirmingDelete = null; draw(); } }),
    );
    item.classList.add("confirming");
    queueMicrotask(() => actions.querySelector<HTMLElement>("[data-focus]")?.focus({ preventScroll: true }));
  } else {
    actions.append(
      h("button", { type: "button", class: "sm", text: t("agents.edit"), onclick: () => openWizard(a) }),
      h("button", { type: "button", class: "danger sm", text: t("agents.delete"), onclick: () => {
        ag.confirmingDelete = a.id;
        ag.flash = null;
        draw();
      } }),
    );
  }
  item.append(actions, sw);
  return item;
}

// ── Roadeep agents ────────────────────────────────────────────────────────────

function roadeepGroup(): HTMLElement {
  const rd = roadeepData();
  const group = h("div", { class: "group card", role: "group", "aria-labelledby": "grp-roadeep" },
    h("div", { class: "subhead" }, h("h3", { id: "grp-roadeep", text: t("agents.roadeepTitle") })),
  );
  if (rd.session?.signedIn !== true) {
    group.append(h("div", { class: "hint", text: t("agents.roadeepSignedOut") }));
  } else if (rd.status?.locks.agents) {
    group.append(h("div", { class: "lock" },
      h("span", { class: "hint", text: t("agents.roadeepLocked") }),
      linkButton(t("account.upgrade"), openRoadeepSite)));
  } else if (rd.catalogError) {
    group.append(h("div", { class: "notice err", text: t("agents.roadeepFailed", { err: rd.catalogError }) }));
  } else if (rd.catalog == null) {
    group.append(h("div", { class: "hint", text: t("settings.loading") }));
  } else if (!rd.catalog.exclusive.length && !rd.catalog.public.length) {
    group.append(h("div", { class: "hint", text: t("agents.roadeepEmpty") }));
  } else {
    group.append(
      h("h4", { text: t("agents.exclusiveTitle") }),
      rd.catalog.exclusive.length
        ? roadeepList(rd.catalog.exclusive)
        : h("div", { class: "hint", text: t("agents.exclusiveEmpty") }),
      h("h4", { text: t("agents.publicTitle") }),
      rd.catalog.public.length
        ? roadeepList(rd.catalog.public)
        : h("div", { class: "hint", text: t("agents.publicEmpty") }),
    );
  }
  return group;
}

function roadeepList(agents: RoadeepAgent[]): HTMLElement {
  const list = h("ul", { class: "agent-list" });
  for (const a of agents) list.append(roadeepRow(a));
  return list;
}

function setRoadeepColor(id: string, color: string | null) {
  const s = host.settings();
  const next = { ...(s.agentColors ?? {}) };
  if (color) next[id] = color;
  else delete next[id];
  s.agentColors = next;
  void host.save();
  draw();
}

function roadeepRow(a: RoadeepAgent): HTMLElement {
  const chosen = host.settings().agentColors?.[a.id];
  const color = agentColor(a.id, chosen);
  const open = ag.colorOpen === a.id;
  const dot = h("button", {
    type: "button", class: "agent-swatch btn", style: `--c:${color}`,
    "aria-label": t("agents.pickColor", { name: a.title }), title: t("agents.pickColor", { name: a.title }),
    "aria-expanded": open ? "true" : "false",
    onclick: () => {
      ag.colorOpen = open ? null : a.id;
      draw();
    },
  });
  const item = h("li", { class: "agent-item" },
    dot,
    h("div", { class: "agent-text" },
      h("div", { class: "agent-name", dir: "auto", text: a.title }),
      a.shortDescription ? h("div", { class: "hint", dir: "auto", text: a.shortDescription }) : null,
    ),
    pillSwitch(agentPillId("roadeep", a.id), t("pills.showNamed", { name: a.title })),
  );
  if (open) {
    const swatches = h("div", { class: "swatches small", role: "radiogroup", "aria-label": t("wizard.color") });
    for (const c of AGENT_PALETTE) {
      const on = c === color;
      swatches.append(h("button", {
        type: "button", class: on ? "swatch on" : "swatch", role: "radio", "aria-checked": on ? "true" : "false",
        "aria-label": c, title: c, style: `--c:${c}`, onclick: () => setRoadeepColor(a.id, c),
      }));
    }
    const reset = h("button", { type: "button", class: "link", text: t("agents.colorAuto"), onclick: () => setRoadeepColor(a.id, null) }) as HTMLButtonElement;
    reset.disabled = !isHexColor(chosen);
    item.append(h("div", { class: "color-pop" }, swatches, reset));
    queueMicrotask(() => item.querySelector<HTMLElement>(".swatch.on")?.focus({ preventScroll: true }));
  }
  return item;
}
