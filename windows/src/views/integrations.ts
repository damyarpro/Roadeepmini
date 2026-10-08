import { CODING_PROVIDERS, isCodingProvider } from "../activity/providers";
// Integration cards shown in the overview's left card — DOM ports of
// IntegrationCardView and friends from IslandViewContent.swift.
//
// Cal.com is the one simplification: macOS shows a three-level calendar
// (month → day → booking); here it is the list of upcoming bookings.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { State, isHexColor, type AgentTask } from "../core/state";
import { Bridge, LOCAL_AGENT_PREFIX } from "../core/bridge";
import { BridgeShortcuts } from "../core/bridge-shortcuts";
import { dateLocale, formatNumber, t } from "../core/i18n";
import { localizeError } from "../core/error-text";
import { catalogOpenUrl, isWebUrl } from "../core/catalog";
import "../core/locales/activity";

/** Same shape as the Swift `timeAgo` computed properties. */
export function timeAgo(value: unknown): string {
  const date = typeof value === "number" ? new Date(value) : new Date(String(value));
  const diff = (Date.now() - date.getTime()) / 1000;
  if (!Number.isFinite(diff)) return "";
  if (diff < 60) return t("time.justNow");
  if (diff < 3600) return t("time.minutes", { n: Math.floor(diff / 60) });
  if (diff < 86400) return t("time.hours", { n: Math.floor(diff / 3600) });
  return t("time.days", { n: Math.floor(diff / 86400) });
}

function header(color: string, name: string, kind: string, extra?: Node): HTMLElement {
  const row = h("div", { class: "int-head" }, dot(color, 7), h("b", { dir: "auto", text: name }), h("span", { text: kind }));
  if (extra) row.append(extra);
  return row;
}

/** Highlighted first row + plain rows, the layout every list card shares. */
function listRow(accent: string, first: boolean, ...children: Node[]): HTMLElement {
  const row = h("div", { class: first ? "int-row first" : "int-row" }, dot(accent, 5), ...children);
  if (first) row.style.background = `${accent}14`;
  return row;
}

function get(id: string): Record<string, unknown> {
  return (State.integrations[id]?.data ?? {}) as Record<string, unknown>;
}

function arr(id: string, key: string): Record<string, unknown>[] {
  const v = get(id)[key];
  return Array.isArray(v) ? (v as Record<string, unknown>[]) : [];
}

// ── Not configured / idle ─────────────────────────────────────────────────────

const OPEN_URLS: Record<string, string> = {
  integration_resend: "https://resend.com/emails",
  integration_vercel: "https://vercel.com/dashboard",
  integration_github: "https://github.com",
  integration_stripe: "https://dashboard.stripe.com/payments",
  integration_notion: "https://notion.so",
  integration_calcom: "https://app.cal.com/bookings",
  integration_linear: "https://linear.app",
  integration_netlify: "https://app.netlify.com",
};

/**
 * GitLab, Sentry, Cloudflare and catalog services: the poller sends the page of
 * the user's instance, region or account. Until it has, a catalog service
 * offers its manifest's fixed page.
 */
function openUrlOf(id: string): string | null {
  const fixed = OPEN_URLS[id];
  if (fixed) return fixed;
  const fromData = get(id).openUrl;
  return isWebUrl(fromData) ? fromData : catalogOpenUrl(id);
}

function idleCard(task: AgentTask, openSettings: () => void): HTMLElement {
  const info = State.integrations[task.id];
  const configured = info?.configured ?? false;
  const error = info?.error ? localizeError(info.error) : null;
  // The Claude Code pill is about hooks, not a key — the macOS wording would be
  // misleading here.
  const missing = t(isCodingProvider(task.id.replace(/^integration_/, "")) ? "int.hooksNotInstalled" : "int.keyNotConfigured");
  const label = error ?? (configured ? t("int.connectedLoading") : missing);
  const statusColor = error || !configured ? "#F4505E" : "#22C55E";

  const actions = h("div", { class: "int-actions" });
  if (task.id === "integration_claude") {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}b3`,
        text: t("int.openVSCode"),
        onclick: () => void BridgeShortcuts.openSession(task.sessionId ?? null, task.sessionCwd ?? null),
      }),
    );
  } else if (task.id === "integration_n8n") {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("int.openN8n"),
        onclick: () => void Bridge.openN8n(),
      }),
    );
  } else if (openUrlOf(task.id)) {
    const url = openUrlOf(task.id)!;
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("int.open", { name: task.name }),
        onclick: () => void Bridge.openUrl(url),
      }),
    );
  }
  if (configured) {
    actions.append(
      h("button", {
        class: "link-btn",
        style: `color:${task.color}d9`,
        text: t("common.refresh"),
        onclick: () => void Bridge.refreshIntegration(task.id),
      }),
    );
  } else {
    actions.append(
      h("button", { class: "link-btn", style: "color:#8e939c", text: t("common.settingsEllipsis"), onclick: openSettings }),
    );
  }

  return h(
    "div",
    { class: "int-card" },
    header(task.color, task.id === "integration_claude" ? t("settings.appName") : task.name, t("int.integration")),
    h("div", { class: "int-status" }, dot(statusColor, 5), h("span", { text: label })),
    actions,
  );
}

// ── Vercel ────────────────────────────────────────────────────────────────────

function vercelCard(onDetail: () => void): HTMLElement {
  const deployments = arr("integration_vercel", "deployments");
  const rows = h("div", { class: "int-rows" });
  deployments.slice(0, 3).forEach((d, i) => {
    const accent = d.state === "READY" ? "#22C55E" : "#F4505E";
    const name = h("span", { class: "int-name", text: String(d.projectName ?? "") });
    const ago = h("span", { class: "int-ago", text: timeAgo(d.createdAt) });
    if (i === 0) {
      const more = h(
        "button",
        { class: "int-more", title: t("int.details"), onclick: onDetail },
        svg(ICONS.ellipsis, 8),
      );
      rows.append(listRow(accent, true, name, ago, more));
    } else {
      rows.append(listRow(accent, false, name, ago));
    }
  });
  return h("div", { class: "int-card" }, header("#7C5CFF", "Vercel", t("int.deployments")), rows);
}

function vercelDetail(onBack: () => void): HTMLElement {
  const d = arr("integration_vercel", "deployments")[0] ?? {};
  const success = d.state === "READY";
  const accent = success ? "#22C55E" : "#F4505E";
  const status = t(success ? "int.ready" : d.state === "CANCELED" ? "int.canceled" : "int.error");
  const body = h("div", { class: "int-detail-body" });
  if (d.commitMessage) body.append(h("div", { class: "int-commit", text: String(d.commitMessage) }));
  const meta = h("div", { class: "int-meta" });
  if (d.branch) meta.append(h("span", { text: String(d.branch) }));
  const ago = timeAgo(d.createdAt);
  // "just now" already says when; "just now ago" doesn't read in any language.
  meta.append(h("span", { text: ago === t("time.justNow") ? ago : t("time.ago", { t: ago }) }));
  body.append(meta);
  if (d.url) {
    body.append(
      h("button", {
        class: "int-link",
        text: String(d.url),
        onclick: () => void Bridge.openUrl(`https://${d.url}`),
      }),
    );
  }
  return h(
    "div",
    { class: "int-card detail" },
    h(
      "div",
      { class: "int-detail-head" },
      h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })),
      dot(accent, 6),
      h("b", { text: String(d.projectName ?? t("int.deployment")) }),
      h("span", { class: "int-badge", style: `color:${accent};background:${accent}24`, text: status }),
    ),
    body,
  );
}

// ── Resend ────────────────────────────────────────────────────────────────────

function resendCard(): HTMLElement {
  const emails = arr("integration_resend", "emails");
  const total = get("integration_resend").total;
  const extra =
    total != null
      ? h("span", { class: "int-total" }, h("i", { class: "pulse" }), h("span", { text: typeof total === "number" ? formatNumber(total) : String(total) }))
      : undefined;
  const rows = h("div", { class: "int-rows" });
  emails.slice(0, 3).forEach((e, i) => {
    const delivered = e.lastEvent === "delivered";
    const accent = delivered ? "#22C55E" : "#F4505E";
    const to = Array.isArray(e.to) ? String(e.to[0] ?? "?") : "?";
    const short = to.split("@")[0];
    const cells: Node[] = [
      h("span", { class: "int-name", text: short }),
      h("span", { class: "int-ago", text: timeAgo(e.createdAt) }),
    ];
    if (i === 0 && e.subject) cells.push(h("span", { class: "int-sub", text: String(e.subject) }));
    rows.append(listRow(accent, i === 0, ...cells));
  });
  return h("div", { class: "int-card" }, header("#22C55E", "Resend", t("int.emails"), extra), rows);
}

// ── GitHub ────────────────────────────────────────────────────────────────────

function statRow(icon: string, color: string, label: string, value: string): HTMLElement {
  return h(
    "div",
    { class: "int-stat" },
    h("i", { class: "int-stat-icon", style: `color:${color}` }, svg(icon, 10)),
    h("span", { class: "int-stat-label", text: label }),
    h("span", { class: "int-stat-value", text: value }),
  );
}

function githubCard(): HTMLElement {
  const d = get("integration_github");
  const stars = Number(d.totalStars ?? 0);
  const repos = Number(d.totalRepos ?? 0);
  const fmt = (n: number) =>
    n >= 1000
      ? t("int.thousands", { n: formatNumber(n / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })
      : formatNumber(n);
  return h(
    "div",
    { class: "int-card" },
    header("#F4505E", "GitHub", t("int.overview")),
    h(
      "div",
      { class: "int-stats" },
      statRow(ICONS.star, "#F5A524", t("int.totalStars"), fmt(stars)),
      statRow(ICONS.stack, "#6B7079", t("int.repositories"), formatNumber(repos)),
    ),
  );
}

// ── Stripe ────────────────────────────────────────────────────────────────────

function stripeCard(): HTMLElement {
  const d = get("integration_stripe");
  const money = { minimumFractionDigits: 2, maximumFractionDigits: 2 };
  const balance = formatNumber(Number(d.balance ?? 0) / 100, money);
  const currency = String(d.currency ?? "eur").toUpperCase();
  const rows = h("div", { class: "int-rows tight" });
  for (const p of arr("integration_stripe", "payments")) {
    const success = p.status === "succeeded";
    const accent = success ? "#22C55E" : "#F4505E";
    rows.append(
      h(
        "div",
        { class: "int-row" },
        dot(accent, 5),
        h("span", { class: "int-name", text: String(p.description ?? t("int.payment")) }),
        h("span", {
          class: "int-amount",
          style: "color:#22c55e",
          text: `+${formatNumber(Number(p.amount ?? 0) / 100, money)}`,
        }),
        h("span", { class: "int-ago", text: timeAgo(p.createdAt) }),
      ),
    );
  }
  return h(
    "div",
    { class: "int-card" },
    header("#0570DE", "Stripe", t("int.payments")),
    h("div", { class: "int-balance" }, h("span", { text: balance }), h("i", { text: currency })),
    rows,
  );
}

// ── Notion ────────────────────────────────────────────────────────────────────

function notionCard(): HTMLElement {
  const rows = h("div", { class: "int-rows tight" });
  for (const p of arr("integration_notion", "pages").slice(0, 3)) {
    rows.append(
      h(
        "button",
        {
          class: "int-page",
          onclick: () => {
            if (typeof p.url === "string") void Bridge.openUrl(p.url);
          },
        },
        p.emoji
          ? h("span", { class: "int-emoji", text: String(p.emoji) })
          : h("i", { class: "int-emoji" }, svg(ICONS.doc, 9)),
        h("span", { class: "int-name", text: String(p.title ?? t("int.untitled")) }),
        h("span", { class: "int-ago", text: timeAgo(p.lastEditedAt) }),
      ),
    );
  }
  return h("div", { class: "int-card" }, header("#E8E8E8", "Notion", t("int.recent")), rows);
}

// ── Cal.com ───────────────────────────────────────────────────────────────────

function calcomCard(): HTMLElement {
  const bookings = arr("integration_calcom", "bookings")
    .slice()
    .sort((a, b) => new Date(String(a.start)).getTime() - new Date(String(b.start)).getTime());
  const rows = h("div", { class: "int-rows tight" });
  if (bookings.length === 0) {
    rows.append(h("div", { class: "int-empty", text: t("int.noCalls") }));
  }
  for (const b of bookings.slice(0, 3)) {
    const when = new Date(String(b.start));
    const day = when.toLocaleDateString(dateLocale(), { day: "2-digit", month: "2-digit" });
    const time = when.toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" });
    rows.append(
      h(
        "div",
        { class: "int-row" },
        dot("#C9956A", 4),
        h("span", { class: "int-time", text: `${day} ${time}` }),
        h("span", { class: "int-name", text: String(b.title ?? t("int.meeting")) }),
      ),
    );
  }
  return h("div", { class: "int-card" }, header("#C9956A", "Cal.com", t("int.schedule")), rows);
}

// ── GitLab, Sentry, Linear, Netlify, Cloudflare ───────────────────────────────
//
// Windows-only services, in the layout of the cards above: a header with an
// optional count, then up to three rows, the first one highlighted.

/** The four deploy words the pollers send (ready / error / canceled / building). */
function stateAccent(state: unknown): string {
  return state === "ready" ? "#22C55E" : state === "error" ? "#F4505E" : state === "canceled" ? "#8E939C" : "#EAB308";
}

function totalChip(n: number, more = false): HTMLElement {
  return h("span", { class: "int-total" }, h("span", { text: `${formatNumber(n)}${more ? "+" : ""}` }));
}

/** Titles come from the user's own projects, in any language. */
const titleCell = (text: unknown) => h("span", { class: "int-name", dir: "auto", text: String(text ?? "") });
const subCell = (text: unknown) => h("span", { class: "int-sub", dir: "auto", text: String(text ?? "") });
const agoCell = (when: unknown) => h("span", { class: "int-ago", text: timeAgo(when) });

function rowsOrEmpty(rows: HTMLElement, empty: string): HTMLElement {
  if (!rows.childElementCount) rows.append(h("div", { class: "int-empty", text: t(empty) }));
  return rows;
}

function gitlabCard(task: AgentTask): HTMLElement {
  const id = "integration_gitlab";
  const requests = arr(id, "mergeRequests");
  const latest = arr(id, "pipelines")[0];
  const rows = h("div", { class: "int-rows tight" });
  // The newest pipeline first (it is what changes), then my merge requests.
  if (latest) {
    rows.append(listRow(stateAccent(latest.state), true, titleCell(latest.project), agoCell(latest.updatedAt), subCell(latest.ref)));
  }
  requests.slice(0, latest ? 2 : 3).forEach((mr, i) => {
    rows.append(listRow(task.color, !latest && i === 0, titleCell(mr.title), agoCell(mr.updatedAt)));
  });
  if (latest && requests.length === 0) rows.append(h("div", { class: "int-empty", text: t("int.noMergeRequests") }));
  const count = Number(get(id).openCount ?? requests.length);
  return h("div", { class: "int-card" },
    header(task.color, "GitLab", t("int.mergeRequests"), totalChip(count)),
    rowsOrEmpty(rows, "int.noMergeRequests"));
}

function sentryCard(task: AgentTask): HTMLElement {
  const id = "integration_sentry";
  const d = get(id);
  const rows = h("div", { class: "int-rows tight" });
  arr(id, "issues").slice(0, 3).forEach((issue, i) => {
    const level = String(issue.level ?? "error");
    const accent = level === "fatal" || level === "error" ? "#F4505E" : level === "warning" ? "#EAB308" : "#8E939C";
    const cells = [titleCell(issue.title), agoCell(issue.firstSeen)];
    if (i === 0 && issue.shortId) cells.push(subCell(issue.shortId));
    rows.append(listRow(accent, i === 0, ...cells));
  });
  return h("div", { class: "int-card" },
    header(task.color, "Sentry", t("int.unresolved"), totalChip(Number(d.unresolved ?? 0), d.more === true)),
    rowsOrEmpty(rows, "int.noIssues"));
}

function linearCard(task: AgentTask): HTMLElement {
  const id = "integration_linear";
  const d = get(id);
  const rows = h("div", { class: "int-rows tight" });
  arr(id, "issues").slice(0, 3).forEach((issue, i) => {
    const stateColor = String(issue.stateColor ?? "");
    const accent = isHexColor(stateColor) ? stateColor : task.color;
    const cells = [titleCell(issue.title), agoCell(issue.createdAt)];
    if (i === 0 && issue.identifier) cells.push(subCell(issue.identifier));
    rows.append(listRow(accent, i === 0, ...cells));
  });
  return h("div", { class: "int-card" },
    header(task.color, "Linear", t("int.assignedToMe"), totalChip(Number(d.total ?? 0), d.more === true)),
    rowsOrEmpty(rows, "int.noAssigned"));
}

/** Netlify deploys and Cloudflare Pages deployments: name, when, and the branch on the newest. */
function deployCard(task: AgentTask, key: string, nameKey: string): HTMLElement {
  const rows = h("div", { class: "int-rows tight" });
  arr(task.id, key).slice(0, 3).forEach((d, i) => {
    const cells = [titleCell(d[nameKey]), agoCell(d.createdAt)];
    if (i === 0 && d.branch) cells.push(subCell(d.branch));
    rows.append(listRow(stateAccent(d.state), i === 0, ...cells));
  });
  return h("div", { class: "int-card" }, header(task.color, task.name, t("int.deployments")), rowsOrEmpty(rows, "int.noDeploys"));
}

// ── Catalog services ──────────────────────────────────────────────────────────
//
// Every manifest-driven service sends the same `{ kind: "list" }` payload
// (src-tauri/src/catalog), so one card serves them all.

/** The engine's five mapped statuses, in the island's existing hues. */
const STATUS_ACCENT: Record<string, string> = {
  ok: "#22C55E", info: "#38BDF8", warn: "#EAB308", err: "#F4505E", off: "#8E939C",
};

/** The raw API status ("ACTIVE_HEALTHY", "failed"), tinted like the deploy badges. */
function statusChip(text: string, accent: string): HTMLElement {
  return h("span", {
    class: "int-badge", dir: "auto", text,
    style: `color:${accent};background:${accent}24;max-width:34%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap`,
  });
}

function catalogListCard(task: AgentTask): HTMLElement {
  const d = get(task.id);
  const items = arr(task.id, "items").filter((item) => typeof item.title === "string");
  const count = typeof d.count === "number" ? d.count : items.length;
  const rows = h("div", { class: "int-rows tight" });
  items.slice(0, 3).forEach((item, i) => {
    const accent = STATUS_ACCENT[String(item.status)] ?? STATUS_ACCENT.info;
    const cells: Node[] = [dot(accent, 5), titleCell(item.title)];
    const statusText = typeof item.statusText === "string" ? item.statusText : "";
    // The row is ~200 px: the subtitle only fits on the first row, and only without a status chip.
    if (i === 0 && !statusText && typeof item.subtitle === "string" && item.subtitle) cells.push(subCell(item.subtitle));
    if (statusText) cells.push(statusChip(statusText, accent));
    if (typeof item.time === "number") cells.push(agoCell(item.time));
    const cls = i === 0 ? "int-row first" : "int-row";
    // Only http(s) links reach the browser; anything else stays a plain row.
    const row = isWebUrl(item.url)
      ? h("button", { type: "button", class: `${cls} int-page`, onclick: () => void Bridge.openUrl(item.url as string) }, ...cells)
      : h("div", { class: cls }, ...cells);
    if (i === 0) row.style.background = `${accent}14`;
    rows.append(row);
  });
  return h("div", { class: "int-card" },
    header(task.color, task.name, t("int.recent"), totalChip(count, d.more === true)),
    rowsOrEmpty(rows, "int.noItems"));
}

// ── n8n ───────────────────────────────────────────────────────────────────────

function n8nCard(task: AgentTask, onDetail: () => void, openSettings: () => void): HTMLElement {
  const hasActivity = task.steps.length > 0 && (task.state === "finished" || task.state === "error");
  if (!hasActivity) return idleCard(task, openSettings);
  const success = task.state === "finished";
  const accent = success ? "#22C55E" : "#F4505E";
  return h(
    "div",
    { class: "int-card" },
    header("#F29B38", "n8n", t("int.workflow")),
    h(
      "div",
      { class: "int-actions" },
      h(
        "button",
        {
          class: "int-pill",
          style: `background:${accent}1a;border-color:${accent}38`,
          onclick: onDetail,
        },
        dot(accent, 5),
        h("span", { class: "int-name", text: task.steps[0] ?? t("int.workflow") }),
        svg(ICONS.ellipsis, 8),
      ),
    ),
  );
}

function n8nDetail(task: AgentTask, onBack: () => void): HTMLElement {
  const success = task.state === "finished";
  const accent = success ? "#22C55E" : "#F4505E";
  const detail = task.steps[1];
  return h(
    "div",
    { class: "int-card detail" },
    h(
      "div",
      { class: "int-detail-head" },
      h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })),
      dot(accent, 6),
      h("b", { text: task.steps[0] ?? t("int.workflow") }),
      h("span", {
        class: "int-badge",
        style: `color:${accent};background:${accent}24`,
        text: t(success ? "int.success" : "int.failed"),
      }),
    ),
    detail
      ? h("pre", { class: "int-detail-text", text: localizeError(detail) })
      : h("div", {
          class: "int-status",
          text: t(success ? "int.completed" : "int.noErrorDetails"),
        }),
  );
}

// ── Agent pills ───────────────────────────────────────────────────────────────

/** The description an agent pill's card shows, from whichever list holds it. */
function agentDescription(ref: string): string {
  const r = State.roadeep;
  if (ref.startsWith(LOCAL_AGENT_PREFIX)) {
    const id = ref.slice(LOCAL_AGENT_PREFIX.length);
    return r.localAgents.find((a) => a.id === id)?.description ?? "";
  }
  return r.agents.find((a) => a.id === ref)?.shortDescription ?? "";
}

function agentCard(task: AgentTask, hooks: IntegrationCardHooks): HTMLElement {
  const ref = task.agentRef ?? "";
  const description = agentDescription(ref);
  const kind = ref.startsWith(LOCAL_AGENT_PREFIX) ? t("int.myAgent") : t("int.roadeepAgent");
  return h(
    "div",
    { class: "int-card" },
    header(task.color, task.name, kind),
    h("div", { class: "int-status agent-desc", dir: "auto", text: description || t("int.agentNoDescription") }),
    h(
      "div",
      { class: "int-actions" },
      h("button", {
        class: "link-btn",
        style: `color:${task.color}`,
        text: t("int.chatWith", { name: task.name }),
        onclick: () => hooks.openAgent(),
      }),
      h("button", { class: "link-btn", style: "color:#8e939c", text: t("common.settingsEllipsis"), onclick: hooks.openSettings }),
    ),
  );
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

export interface IntegrationCardHooks {
  detailOpen: boolean;
  openDetail(): void;
  closeDetail(): void;
  openSettings(): void;
  /** Agent pills: open the chat with the focused agent. */
  openAgent(): void;
}

/** True when this integration has data worth showing instead of the idle card. */
export function hasIntegrationData(id: string): boolean {
  const info = State.integrations[id];
  if (!info || info.error) return false;
  switch (id) {
    case "integration_vercel":
      return arr(id, "deployments").length > 0;
    case "integration_resend":
      return arr(id, "emails").length > 0;
    case "integration_github":
      return get(id).totalRepos != null;
    case "integration_stripe":
      return info.loaded;
    case "integration_notion":
      return arr(id, "pages").length > 0;
    case "integration_calcom":
    case "integration_gitlab":
    case "integration_sentry":
    case "integration_linear":
    case "integration_netlify":
    case "integration_cloudflare":
      return info.loaded;
    default:
      return info.loaded && get(id).kind === "list";
  }
}

export function renderIntegrationCard(task: AgentTask, hooks: IntegrationCardHooks): HTMLElement {
  if (task.id === "integration_codex") return h("div", {class:"int-card"},
    header(task.color,task.name,"Codex"),h("p",{class:"int-idle",text:t("activity.local")}),
    h("button",{type:"button",class:"link-btn",text:t("activity.openCodex"),onclick:()=>hooks.openAgent()}));
  const provider = task.id.replace(/^integration_/, "");
  if (isCodingProvider(provider) && provider !== "claude") return h("div", {class:"int-card"},
    header(task.color, task.name, CODING_PROVIDERS[provider]),
    h("p",{class:"int-idle",text:t("activity.local")}),
    h("button",{type:"button",class:"link-btn",text:t("nav.settings"),onclick:()=>void Bridge.openSettingsWindow("mcp")}));
  if (task.agentRef) return agentCard(task, hooks);
  if (task.id === "integration_n8n") {
    const hasActivity = task.steps.length > 0 && (task.state === "finished" || task.state === "error");
    return hooks.detailOpen && hasActivity
      ? n8nDetail(task, hooks.closeDetail)
      : n8nCard(task, hooks.openDetail, hooks.openSettings);
  }
  if (task.id === "integration_vercel" && hasIntegrationData(task.id)) {
    return hooks.detailOpen ? vercelDetail(hooks.closeDetail) : vercelCard(hooks.openDetail);
  }
  if (!hasIntegrationData(task.id)) return idleCard(task, hooks.openSettings);

  switch (task.id) {
    case "integration_resend":
      return resendCard();
    case "integration_github":
      return githubCard();
    case "integration_stripe":
      return stripeCard();
    case "integration_notion":
      return notionCard();
    case "integration_calcom":
      return calcomCard();
    case "integration_gitlab":
      return gitlabCard(task);
    case "integration_sentry":
      return sentryCard(task);
    case "integration_linear":
      return linearCard(task);
    case "integration_netlify":
      return deployCard(task, "deploys", "site");
    case "integration_cloudflare":
      return deployCard(task, "deployments", "project");
    default:
      return get(task.id).kind === "list" ? catalogListCard(task) : idleCard(task, hooks.openSettings);
  }
}

export { clear };
