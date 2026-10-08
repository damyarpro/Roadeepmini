import { Bridge } from "../core/bridge";
import { formatNumber, t } from "../core/i18n";
import { State } from "../core/state";
import { recapMarkdown } from "../activity/recap";
import { diffPreview } from "../activity/diff";
import type { ActivitySession, GitInspection, HandoffAgent } from "../activity/types";
import { h } from "./dom";

/** Show only observations. Wallet credit and cumulative tokens are unrelated. */
export function observedUsage(session: ActivitySession): HTMLElement | null {
  if (!session.context && !session.usage) return null;
  const section = h("section", { class: "act-usage", "aria-label": t("activity.usage") });
  const c = session.context;
  if (c) {
    const measured = c.usedPercent != null ? `${formatNumber(Math.round(c.usedPercent))}%`
      : c.usedTokens != null && c.limitTokens != null ? `${formatNumber(c.usedTokens)} / ${formatNumber(c.limitTokens)}` : t("activity.unknown");
    section.append(h("span", { title: c.model || "" }, h("span", {class:"act-usage-label",text:`${t("activity.context")} · `}), h("strong", {text:measured})));
  }
  for (const [name, window] of Object.entries(session.usage ?? {})) {
    if (!window) continue;
    const row = h("span", {}, h("span", {class:"act-usage-label",text:`${t(name === "primary" ? "activity.quotaPrimary" : "activity.quotaSecondary")} · `}),
      h("strong", {text:`${formatNumber(Math.round(window.usedPercent))}%`}), ` ${t("activity.used")}`);
    if (window.resetsAt != null) row.title = t("activity.resets", { at: new Date(window.resetsAt).toLocaleString(State.settings.language === "fa" ? "fa-IR" : "en-US") });
    section.append(row);
  }
  section.append(h("small", { text: t("activity.usageObserved") }));
  return section;
}

interface ActionHost {
  git(cwd: string): Promise<GitInspection>;
  agents(): Promise<HandoffAgent[]>;
  handoff(cwd: string, agent: string, content: string): Promise<{ path: string; agent: string }>;
  changed(): void;
  log(message: string): void;
}

/** Git and terminal access happen only in click handlers, never in sync or polling. */
export function createCodingActions(host: ActionHost = {
  git: Bridge.codingGitInspect, agents: Bridge.codingHandoffAgents, handoff: Bridge.codingHandoff,
  changed: () => State.notify(), log: (message) => { void Bridge.log(message); },
}) {
  let session: ActivitySession | undefined;
  let pending = false;
  let request = 0;
  let shownRoot = "";
  let agents: HandoffAgent[] = [];
  const gitByRoot = new Map<string, GitInspection>();
  const status = h("p", { class: "act-action-status", role: "status", "aria-live": "polite" });
  const gitResult = h("div", { class: "act-git-result" });
  const agentPick = h("select", { class: "act-select", "data-act-focus":"agent", "aria-label": t("activity.handoffAgent") });
  const launch = h("button", { type: "button", class: "act-clear", "data-act-focus":"launch", text: t("activity.handoffLaunch") });
  const terminal = h("div", { class: "act-terminal", hidden: true },
    h("p", { text: t("activity.handoffHint") }), agentPick, launch);
  const inspect = h("button", { type: "button", class: "act-clear", "data-act-focus":"git", text: t("activity.gitInspect") });
  const prepare = h("button", { type: "button", class: "act-clear", "data-act-focus":"prepare", text: t("activity.handoffPrepare") });
  const el = h("details", { class: "act-project-tools" }, h("summary", { "data-act-focus":"summary", text: t("activity.projectTools") }),
    h("p", { class: "act-disclaimer", text: t("activity.gitHint") }),
    h("div", { class: "act-action-row" }, inspect, prepare), status, terminal, gitResult);

  function lock() {
    inspect.disabled = prepare.disabled = pending || !session?.cwd;
    launch.disabled = pending || !session?.cwd || !agents.length;
    agentPick.disabled = pending;
  }
  function renderGit() {
    gitResult.replaceChildren();
    const result = gitByRoot.get(session?.cwd ?? "");
    if (!result) return;
    gitResult.append(h("p", { class: "act-path", dir: "ltr", text: result.root }),
      h("small", { text: t("activity.gitSnapshot", { at: new Date(result.at).toLocaleTimeString(State.settings.language === "fa" ? "fa-IR" : "en-US") }) }));
    if (!result.files.length) gitResult.append(h("p", { text: t("activity.gitClean") }));
    for (const file of result.files) gitResult.append(h("div", { class: "act-file", dir: "ltr", text: `${file.status} · ${file.path}` }));
    if (result.patch) {
      const details = h("details", { class: "act-diff" }, h("summary", { "data-act-focus":"git-summary", text: t("activity.gitDiff") }));
      const pre = h("pre", { dir: "ltr", tabindex: "0", "data-act-focus":"git-patch", "aria-label": t("activity.gitDiff") });
      const preview = diffPreview(result.patch);
      for (const line of preview.lines) pre.append(h("div", { class: `act-line ${line.kind}`, text: line.text }));
      details.append(pre);
      if (result.truncated || preview.truncated) details.append(h("small", { text: t("activity.truncated") }));
      gitResult.append(details);
    }
  }
  inspect.addEventListener("click", async () => {
    if (pending || !session?.cwd) return;
    const cwd = session.cwd;
    const generation = ++request;
    pending = true; lock(); status.textContent = t("activity.gitLoading");
    try {
      const result = await host.git(cwd);
      if (generation !== request) return;
      gitByRoot.set(cwd, result);
      if (session?.cwd === cwd) { renderGit(); status.textContent = t("activity.gitLoaded"); }
    } catch {
      if (generation === request) status.textContent = t("activity.gitFailed");
      host.log("coding activity git inspection failed");
    } finally { pending = false; lock(); host.changed(); }
  });
  prepare.addEventListener("click", async () => {
    if (pending || !session?.cwd) return;
    const generation = ++request;
    pending = true; lock(); status.textContent = t("activity.handoffLoading");
    try {
      const result = await host.agents();
      if (generation !== request) return;
      agents = result;
      agentPick.replaceChildren(...agents.map((agent) => h("option", { value: agent.id, text: agent.name })));
      terminal.hidden = agents.length === 0;
      status.textContent = agents.length ? "" : t("activity.handoffNoAgents");
    } catch {
      if (generation === request) status.textContent = t("activity.handoffFailed");
      host.log("coding activity handoff agent discovery failed");
    } finally { pending = false; lock(); host.changed(); }
  });
  launch.addEventListener("click", async () => {
    if (pending || !session?.cwd || !agents.some((agent) => agent.id === agentPick.value)) return;
    const captured = session;
    pending = true; lock(); status.textContent = t("activity.handoffLoading");
    try {
      await host.handoff(captured.cwd!, agentPick.value, recapMarkdown(captured, State.settings.language));
      status.textContent = t("activity.handoffOpened");
    } catch { status.textContent = t("activity.handoffFailed"); host.log("coding activity terminal handoff failed"); }
    finally { pending = false; lock(); host.changed(); }
  });
  return {
    el,
    sync(next: ActivitySession | undefined) {
      if ((next?.cwd ?? "") !== shownRoot) {
        request++;
        shownRoot = next?.cwd ?? "";
        status.textContent = "";
        terminal.hidden = true;
        agents = [];
        session = next;
        renderGit();
      }
      session = next;
      lock();
    },
  };
}
