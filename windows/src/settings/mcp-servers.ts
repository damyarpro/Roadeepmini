// Settings → MCP servers: the external MCP servers whose tools the island chat
// assistant may use (the app is the MCP client; src-tauri/src/mcpc/). One row
// per server: status, on/off, and when opened its keys or sign-in, the exact
// command a local server runs (approved by an explicit click), a connection
// test and a mode per tool. Adding goes through mcp-servers-add.ts.
//
// Everything a server sends (names, tool descriptions, errors) is untrusted
// text: it is cleaned, capped and only ever set as textContent.

import "./mcp-servers.css";
import { isolate, isRtl, registerMessages, t } from "../core/i18n";
import { mcpcEn } from "../core/locales/mcpc-en";
import { mcpcFa } from "../core/locales/mcpc-fa";
import { Bridge } from "../core/bridge";
import { errorCode, localizeError } from "../core/error-text";
import {
  BridgeMcpc, onMcpcStatus,
  type McpcDirectoryEntry, type McpcSecretSlot, type McpcServerView, type McpcStatusEvent, type McpcToolMode, type McpcToolView,
} from "../core/bridge-mcpc";
import type { Settings } from "../core/state";
import { h, clear } from "../views/dom";
import { followTextDirection, icon, sectionHead, settingRow, statusBadge, switchEl } from "./ui";
import { cleanText, commandLine, loc, openAddDialog, urlHost } from "./mcp-servers-add";

registerMessages(mcpcEn, mcpcFa);

// ── State (module level, so a redraw on a language change keeps it) ─────────

let servers: McpcServerView[] | null = null;
let loadError: string | null = null;
let directory: McpcDirectoryEntry[] | null = null;
let directoryError: string | null = null;
let loading = false;
let subscribed = false;

/** Open/closed per server; a server that needs something opens by itself. */
const expanded = new Map<string, boolean>();
/** The last tool list each server gave, with the modes as set here. */
const toolLists = new Map<string, McpcToolView[]>();
/** The tool filter typed per server. */
const toolQueries = new Map<string, string>();
/** A message to show once at the top of a server's panel (e.g. a key not saved on add). */
const rowNotices = new Map<string, string>();
/** Live rows, by server id; replaced on every full draw. */
const rows = new Map<string, { el: HTMLElement; sync: () => void }>();

/** The section currently on screen (the last one built). */
let view: { badge: HTMLElement; card: HTMLElement } | null = null;

/** The settings window's own settings and save path (main.ts). */
export interface McpServersHost {
  settings: () => Settings;
  save: () => Promise<void>;
}

const TOOL_SEARCH_FROM = 8;
const ERROR_MAX = 2000;

/** One polite live region, kept across redraws so a message survives one. */
const live = h("span", { class: "sr-only", role: "status", "aria-live": "polite" });
function announce(text: string) {
  live.textContent = "";
  window.setTimeout(() => { live.textContent = text; }, 50);
}

const reduceMotion = () => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
const serverOf = (id: string) => servers?.find((s) => s.id === id) ?? null;
const nameOf = (s: McpcServerView) => cleanText(s.name, 60) || s.id;
/** Server ids are [a-z0-9-] by contract; this keeps a DOM id valid whatever arrives. */
const domId = (id: string) => id.replace(/[^A-Za-z0-9_-]/g, "-");
const entryOf = (s: McpcServerView) =>
  s.source.startsWith("directory:") ? directory?.find((e) => e.id === s.source.slice("directory:".length)) ?? null : null;
const errorText = (err: unknown) => {
  const text = localizeError(err);
  return text.length > ERROR_MAX ? `${text.slice(0, ERROR_MAX - 1)}…` : text;
};

// ── Loading and live updates ─────────────────────────────────────────────────

async function load() {
  if (loading) return;
  loading = true;
  const [list, dir] = await Promise.allSettled([BridgeMcpc.list(), BridgeMcpc.directory()]);
  loading = false;
  if (list.status === "fulfilled") {
    servers = list.value;
    loadError = null;
  } else {
    servers = null;
    loadError = String(list.reason);
  }
  if (dir.status === "fulfilled") {
    directory = dir.value;
    directoryError = null;
  } else {
    directory = null;
    directoryError = String(dir.reason);
  }
  draw();
}

/** Re-reads the list; rows update in place unless servers came or went. */
async function refreshList(): Promise<void> {
  let fresh: McpcServerView[];
  try {
    fresh = await BridgeMcpc.list();
  } catch (err) {
    void Bridge.log(`settings: mcp servers refresh failed: ${String(err).split(/[|\n]/, 1)[0]}`);
    return;
  }
  const sameIds = !!servers && servers.length === fresh.length && servers.every((s, i) => s.id === fresh[i].id);
  servers = fresh;
  if (sameIds) {
    for (const row of rows.values()) row.sync();
    setHeadBadge();
  } else {
    draw();
  }
}

let refreshTimer = 0;
/** Several events in a row (connect → ready) cost one re-read. */
function refreshSoon() {
  window.clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(() => void refreshList(), 300);
}

function onStatus(e: McpcStatusEvent) {
  const s = serverOf(e.id);
  if (!s) {
    refreshSoon();
    return;
  }
  s.status = e.status;
  s.error = e.error;
  // Turned off: its list may be stale when it comes back (a stopped idle server's isn't).
  if (e.status === "off") toolLists.delete(e.id);
  rows.get(e.id)?.sync();
  setHeadBadge();
  // Tool count, server info and stored keys come with the full view.
  refreshSoon();
}

function subscribe() {
  if (subscribed) return;
  subscribed = true;
  void onMcpcStatus(onStatus);
}

// ── Section ──────────────────────────────────────────────────────────────────

export function mcpServersSection(host: McpServersHost): HTMLElement {
  const badge = h("span", { class: "sec-badge" });
  const card = h("div", { class: "group card mcpc", role: "group", "aria-labelledby": "grp-mcpc" });
  view = { badge, card };
  subscribe();
  if (servers === null && !loadError) void load();
  draw();
  return h("section", { class: "sec", "aria-labelledby": "sec-mcpc-title" },
    sectionHead({ id: "sec-mcpc-title", icon: "servers", title: t("mcpc.title"), desc: t("mcpc.desc"), badge }),
    chatToolsRow(host),
    card,
  );
}

/** Whether the island chat offers these tools at all (Settings.chatTools). */
function chatToolsRow(host: McpServersHost): HTMLElement {
  return h("div", { class: "card list" },
    settingRow({ label: t("mcpc.chatTools"), hint: t("mcpc.chatToolsHint") },
      switchEl(host.settings().chatTools !== false, false, t("mcpc.chatTools"), (on) => {
        host.settings().chatTools = on;
        void host.save();
        void Bridge.log(`settings: chat tools ${on ? "on" : "off"}`);
      }),
    ),
  );
}

function setHeadBadge() {
  if (!view) return;
  clear(view.badge);
  // What actually works, and what is waiting on the user; "on" alone would overstate it.
  const ready = servers?.filter((s) => s.status === "ready").length ?? 0;
  const waiting = servers?.filter(needsAttention).length ?? 0;
  if (ready > 0) view.badge.append(statusBadge("ok", t("mcpc.readyCount", { n: ready })));
  if (waiting > 0) view.badge.append(statusBadge("warn", waiting === 1 ? t("mcpc.attentionCountOne") : t("mcpc.attentionCount", { n: waiting })));
}

/** "1 tool" / "5 tools". */
const toolCount = (n: number) => (n === 1 ? t("mcpc.toolCountOne") : t("mcpc.toolCount", { n }));

function openDialog() {
  openAddDialog({
    directory,
    directoryError,
    isAdded: (entryId) => !!servers?.some((s) => s.source === `directory:${entryId}`),
    onAdded: (id, name, secretFailed) => void afterAdd(id, name, secretFailed),
  });
}

async function afterAdd(id: string, name: string, secretFailed: boolean) {
  expanded.set(id, true);
  if (secretFailed) rowNotices.set(id, t("mcpc.dlg.secretFailed"));
  await refreshList();
  const item = document.getElementById(`mcpc-item-${domId(id)}`);
  item?.scrollIntoView({ block: "center", behavior: reduceMotion() ? "auto" : "smooth" });
  // Never the approve button: a stray Enter must not confirm a command.
  (item?.querySelector<HTMLElement>(".mcpc-panel input") ?? item?.querySelector<HTMLElement>("button.mcpc-id"))
    ?.focus({ preventScroll: true });
  announce(t("mcpc.added", { name: isolate(cleanText(name, 60)) }) + (secretFailed ? ` ${t("mcpc.dlg.secretFailed")}` : ""));
}

function draw() {
  if (!view) return;
  const { card } = view;
  rows.clear();
  clear(card);
  setHeadBadge();
  // One row that needs something opens by itself (when none is open); the rest
  // keep their warn badge, so the list stays readable.
  if (servers && ![...expanded.values()].some(Boolean)) {
    const first = servers.find((s) => !expanded.has(s.id) && needsAttention(s));
    if (first) expanded.set(first.id, true);
  }

  const addButton = h("button", { type: "button", class: "sm", id: "mcpc-add", onclick: openDialog },
    icon("plus", 14), h("span", { text: t("mcpc.add") })) as HTMLButtonElement;
  card.append(h("div", { class: "subhead" },
    h("h3", { id: "grp-mcpc", text: t("mcpc.myServers") }), h("span", { class: "spacer" }), addButton));

  if (servers === null) {
    addButton.hidden = true;
    if (loadError) {
      card.append(
        h("div", { class: "notice err", role: "alert", dir: "auto", text: t("mcpc.loadFailed", { err: errorText(loadError) }) }),
        h("div", { class: "actions" }, h("button", {
          type: "button", class: "sm", text: t("mcpc.retry"),
          onclick: () => { loadError = null; draw(); void load(); },
        })),
      );
    } else {
      card.append(h("p", { class: "hint", text: t("mcpc.loading") }));
    }
    card.append(live);
    return;
  }

  if (servers.length === 0) {
    addButton.hidden = true;
    card.append(h("div", { class: "empty-card" },
      h("span", { class: "mcpc-empty-art", "aria-hidden": "true" }, icon("servers", 22)),
      h("div", { class: "agent-text" },
        h("div", { class: "agent-name", text: t("mcpc.emptyTitle") }),
        h("div", { class: "hint", text: t("mcpc.empty") }),
      ),
      h("button", { type: "button", class: "primary sm", id: "mcpc-add-empty", onclick: openDialog },
        icon("plus", 16), h("span", { text: t("mcpc.add") })),
    ));
  } else {
    const list = h("ul", { class: "mcpc-list" });
    for (const s of servers) {
      const row = serverRow(s.id);
      rows.set(s.id, row);
      list.append(row.el);
    }
    card.append(h("p", { class: "hint", text: t("mcpc.listHint") }), list);
  }
  card.append(live);
}

// ── One server ───────────────────────────────────────────────────────────────

function badgeFor(s: McpcServerView): HTMLElement {
  switch (s.status) {
    case "ready": return statusBadge("ok", t("mcpc.status.ready"));
    case "needs_auth": return statusBadge("warn", t(s.auth.type === "oauth" ? "mcpc.status.needsSignIn" : "mcpc.status.needsToken"));
    case "needs_approval": return statusBadge("warn", t("mcpc.status.needsApproval"));
    case "error": return statusBadge("err", t("mcpc.status.error"));
    case "connecting": return statusBadge("off", t("mcpc.status.connecting"));
    case "idle": return statusBadge("off", t("mcpc.status.idle"));
    default: return statusBadge("off", t("mcpc.status.off"));
  }
}

/** What a closed row says under the name: the host, or the command it runs. */
function whereText(s: McpcServerView): string {
  return s.transport.type === "http" ? urlHost(s.transport.url) : commandLine(s.transport.command, s.transport.args);
}

/** Off, not approved, or a key / sign-in still missing: nothing could connect yet. */
const canConnect = (s: McpcServerView) =>
  s.enabled && s.status !== "needs_approval" && (s.auth.type === "none" || s.auth.present) && missingEnv(s).length === 0;

/** Environment values a local server declares but has none stored for: all of them are required. */
const missingEnv = (s: McpcServerView) =>
  s.transport.type === "stdio" ? s.transport.env.filter((e) => !e.present).map((e) => e.name) : [];

const needsAttention = (s: McpcServerView) =>
  s.status === "needs_approval" || s.status === "needs_auth" || s.status === "error" || (s.enabled && missingEnv(s).length > 0);

function serverRow(id: string): { el: HTMLElement; sync: () => void } {
  const first = serverOf(id)!;
  const panelId = `mcpc-panel-${domId(id)}`;
  const mark = h("span", { class: "mcpc-mark", "aria-hidden": "true" });
  const name = h("span", { class: "mcpc-name", dir: "auto" });
  const badgeSlot = h("span", { class: "mcpc-badge" });
  const where = h("span", { class: "mcpc-where", dir: "ltr" });
  const count = h("span", { class: "mcpc-count" });
  const disclose = h("button", { type: "button", class: "mcpc-id", "aria-controls": panelId },
    mark,
    h("span", { class: "mcpc-titles" },
      h("span", { class: "mcpc-name-line" }, name, badgeSlot),
      h("span", { class: "mcpc-sub" }, where, count),
    ),
    h("span", { class: "chev", "aria-hidden": "true" }, icon("chevronDown", 16)),
  ) as HTMLButtonElement;

  let switching = false;
  const sw = switchEl(first.enabled, false, t("mcpc.enableNamed", { name: isolate(nameOf(first)) }), (on) => void toggle(on));

  const rowError = h("p", { class: "int-test-err mcpc-row-err", id: `mcpc-row-err-${domId(id)}`, role: "alert", dir: "auto" });
  /** A switch-on refused for missing values: done for the user once the last one is stored. */
  let wantedOn = false;
  const panel = h("div", { class: "mcpc-panel sub-strip", id: panelId });
  const notice = h("div", { class: "notice warn", role: "status", dir: "auto" });
  const errorBox = h("div", { class: "notice err mcpc-error", role: "alert", dir: "auto" });
  const test = testBlock(id);
  const tools = toolsBlock(id);
  const cmd = first.transport.type === "stdio" ? commandBlock(id, test.run) : null;
  const auth = authBlock(id, test.run);
  panel.append(...[notice, errorBox, cmd?.el, auth?.el, test.el, tools.el, removeFoot(id)].filter((x): x is HTMLElement => !!x));

  const el = h("li", { class: "mcpc-item", id: `mcpc-item-${domId(id)}` },
    h("div", { class: "mcpc-head" }, disclose, h("div", { class: "mcpc-ctrls" }, sw)),
    rowError,
    panel,
  );

  async function toggle(on: boolean) {
    const s = serverOf(id);
    if (!s || switching) return;
    const missing = on ? missingEnv(s) : [];
    wantedOn = missing.length > 0;
    if (missing.length) {
      // It couldn't start: say what it needs, where it is entered, and leave it off.
      sw.classList.remove("on");
      sw.setAttribute("aria-checked", "false");
      expanded.set(id, true);
      sync();
      document.getElementById(`mcpc-${domId(id)}-${domId(`env:${missing[0]}`)}`)?.focus({ preventScroll: true });
      return;
    }
    switching = true;
    sw.disabled = true;
    rowError.textContent = "";
    rowError.classList.remove("warn");
    try {
      await BridgeMcpc.update(id, { enabled: on });
      s.enabled = on;
      if (!on) toolLists.delete(id);
      void Bridge.log(`settings: mcp server ${id} ${on ? "on" : "off"}`);
    } catch (err) {
      rowError.textContent = t("mcpc.toggleFailed", { name: isolate(nameOf(s)), err: errorText(err) });
    }
    switching = false;
    sw.disabled = false;
    await refreshList();
    sync();
  }

  function sync() {
    const s = serverOf(id);
    if (!s) return;
    const open = expanded.get(id) ?? false;
    const label = nameOf(s);
    name.textContent = label;
    mark.textContent = Array.from(label.replace(/^[^\p{L}\p{N}]+/u, ""))[0]?.toUpperCase() ?? "•";
    clear(badgeSlot);
    badgeSlot.append(badgeFor(s));
    where.textContent = whereText(s);
    where.title = where.textContent;
    count.textContent = s.status === "ready" ? toolCount(s.toolCount) : "";
    disclose.setAttribute("aria-expanded", open ? "true" : "false");
    disclose.title = t(open ? "mcpc.hideDetails" : "mcpc.showDetails", { name: label });
    panel.hidden = !open;
    if (!switching) {
      sw.classList.toggle("on", s.enabled);
      sw.setAttribute("aria-checked", s.enabled ? "true" : "false");
    }
    const pending = rowNotices.get(id);
    notice.textContent = pending ?? "";
    notice.hidden = !pending;
    errorBox.textContent = s.error && s.status !== "off" ? errorText(s.error) : "";
    errorBox.hidden = !errorBox.textContent;
    // The refused switch-on: named while values are missing, finished once they are stored.
    const missing = missingEnv(s);
    if (wantedOn && missing.length) {
      rowError.textContent = t("mcpc.envMissing", { names: isolate(missing.join(", ")) });
      rowError.classList.add("warn");
    } else if (rowError.classList.contains("warn")) {
      rowError.textContent = "";
      rowError.classList.remove("warn");
    }
    for (const input of panel.querySelectorAll<HTMLInputElement>("input[data-env]")) {
      const flagged = wantedOn && missing.includes(input.dataset.env ?? "");
      if (flagged) {
        input.setAttribute("aria-invalid", "true");
        input.setAttribute("aria-describedby", rowError.id);
      } else {
        input.removeAttribute("aria-invalid");
        input.removeAttribute("aria-describedby");
      }
    }
    if (wantedOn && !missing.length && !switching && !s.enabled) {
      wantedOn = false;
      // Finishes the click that was refused, then shows whether it starts, as approval and sign-in do.
      void toggle(true).then(() => {
        const cur = serverOf(id);
        if (!cur?.enabled) return;
        announce(t("mcpc.enabledAuto", { name: isolate(nameOf(cur)) }));
        if (canConnect(cur)) void test.run();
      });
    }
    cmd?.sync();
    auth?.sync();
    test.sync();
    tools.sync(open);
  }

  disclose.addEventListener("click", () => {
    const s = serverOf(id);
    if (!s) return;
    const open = !(expanded.get(id) ?? false);
    expanded.set(id, open);
    // Seen once is enough.
    rowNotices.delete(id);
    sync();
    if (open) panel.scrollIntoView({ block: "nearest", behavior: "auto" });
  });

  sync();
  return { el, sync };
}

/** A local server: the exact command, and the approval it needs before it ever runs. */
function commandBlock(id: string, runTest: () => Promise<void>): { el: HTMLElement; sync: () => void } {
  const code = h("pre", { class: "mcpc-code", dir: "ltr" });
  const state = h("span", { class: "mcpc-cmd-state" });
  const failure = h("p", { class: "int-test-err", role: "alert", dir: "auto" });
  const approve = h("button", { type: "button", class: "primary sm", text: t("mcpc.approve") }) as HTMLButtonElement;
  const ask = h("div", { class: "mcpc-approve" },
    h("p", { text: t("mcpc.cmdApproveHint") }),
    h("div", { class: "actions" }, approve),
    failure,
  );

  let approving = false;
  // The hash of the command in the box right now: approval is for what was
  // shown, and Rust refuses it if the stored command differs.
  let shownHash: string | null = null;
  approve.addEventListener("click", async () => {
    // aria-disabled rather than disabled, so keyboard focus stays on the button.
    if (approving || !shownHash) return;
    approving = true;
    approve.setAttribute("aria-disabled", "true");
    failure.textContent = "";
    try {
      await BridgeMcpc.approveCommand(id, shownHash);
      void Bridge.log(`settings: mcp server ${id} command approved`);
    } catch (err) {
      failure.textContent = t("mcpc.approveFailed", { err: errorText(err) });
      if (errorCode(err) === "E_MCPC_COMMAND_CHANGED") {
        void Bridge.log(`settings: mcp server ${id} command changed before approval; showing the new one`);
        // Show the command as it is now; the message stays until the next try.
        await refreshList();
      }
      return;
    } finally {
      approving = false;
      approve.removeAttribute("aria-disabled");
    }
    await refreshList();
    // The approval box is gone: focus goes where the story continues.
    const row = document.getElementById(`mcpc-item-${domId(id)}`);
    (row?.querySelector<HTMLElement>(".mcpc-test:not([hidden]) button") ?? row?.querySelector<HTMLElement>("button.mcpc-id"))
      ?.focus({ preventScroll: true });
    // What the user wants to see next is whether it starts, and its tools.
    const s = serverOf(id);
    if (s && canConnect(s)) await runTest();
  });

  const el = h("div", { class: "mcpc-cmd" },
    h("div", { class: "mcpc-cmd-head" }, h("span", { class: "mcpc-label", text: t("mcpc.cmdTitle") }), state),
    code,
    ask,
  );

  const sync = () => {
    const s = serverOf(id);
    if (!s || s.transport.type !== "stdio") return;
    code.textContent = commandLine(s.transport.command, s.transport.args);
    shownHash = s.commandHash;
    ask.hidden = s.status !== "needs_approval";
    clear(state);
    // Off says nothing about approval, so nothing is claimed then.
    if (s.status !== "needs_approval" && s.status !== "off") state.append(statusBadge("ok", t("mcpc.approved")));
  };
  return { el, sync };
}

/** Sign-in (OAuth), the token (bearer/header) or a local server's environment values. */
function authBlock(id: string, runTest: () => Promise<void>): { el: HTMLElement; sync: () => void } | null {
  const s = serverOf(id)!;
  if (s.auth.type === "oauth") return oauthBlock(id, runTest);
  const fields: { el: HTMLElement; sync: () => void }[] = [];
  if (s.auth.type === "bearer" || s.auth.type === "header") {
    const label = s.auth.type === "header" && s.auth.header
      ? t("mcpc.headerValue", { header: isolate(cleanText(s.auth.header, 64)) })
      : t("mcpc.token");
    fields.push(secretField(id, "token", label, () => serverOf(id)?.auth.present ?? false));
  }
  if (s.transport.type === "stdio") {
    const entry = entryOf(s);
    const labels = entry?.transport.type === "stdio" ? entry.transport.env : [];
    for (const env of s.transport.env) {
      const known = labels.find((l) => l.name === env.name);
      const label = known ? loc(known.label) || env.name : env.name;
      fields.push(secretField(id, `env:${env.name}`, label, () => {
        const cur = serverOf(id);
        return cur?.transport.type === "stdio" ? cur.transport.env.find((e) => e.name === env.name)?.present ?? false : false;
      }, { required: true, env: env.name, hint: known ? t("mcpc.dlg.envVar", { name: isolate(env.name) }) : undefined }));
    }
  }
  if (fields.length === 0) return null;
  const help = entryOf(s)?.authHelp;
  const el = h("div", { class: "mcpc-keys" },
    ...fields.map((f) => f.el),
    help ? h("p", { class: "key-help-text", text: loc(help) }) : null,
    h("p", { class: "storage-note", text: t("mcpc.secretsNote") }),
  );
  return { el, sync: () => fields.forEach((f) => f.sync()) };
}

function oauthBlock(id: string, runTest: () => Promise<void>): { el: HTMLElement; sync: () => void } {
  const state = h("span", { class: "mcpc-auth-state" });
  const action = h("span", { class: "mcpc-auth-action" });
  const failure = h("p", { class: "int-test-err", role: "alert", dir: "auto" });
  const hint = h("p", { class: "key-help-text", role: "status", text: t("mcpc.oauthHint") });
  let busy = false;

  const signIn = async (button: HTMLButtonElement) => {
    if (busy) return;
    busy = true;
    button.setAttribute("aria-disabled", "true");
    button.setAttribute("aria-busy", "true");
    hint.textContent = t("mcpc.signingIn");
    failure.textContent = "";
    try {
      await BridgeMcpc.oauthStart(id);
      void Bridge.log(`settings: mcp server ${id} signed in`);
    } catch (err) {
      failure.textContent = t("mcpc.signInFailed", { err: errorText(err) });
    }
    busy = false;
    hint.textContent = t("mcpc.oauthHint");
    await refreshList();
    sync();
    refocus();
    const s = serverOf(id);
    if (s?.auth.present && s.enabled && s.status !== "needs_auth") await runTest();
  };

  const signOut = async () => {
    if (busy) return;
    busy = true;
    failure.textContent = "";
    try {
      await BridgeMcpc.oauthSignout(id);
      toolLists.delete(id);
      void Bridge.log(`settings: mcp server ${id} signed out`);
    } catch (err) {
      failure.textContent = errorText(err);
    }
    busy = false;
    await refreshList();
    sync();
    refocus();
  };

  /** The action button is rebuilt on every sync; keyboard focus follows it. */
  const refocus = () => {
    if (!el.contains(document.activeElement) || document.activeElement === document.body) {
      action.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
    }
  };

  function sync() {
    const s = serverOf(id);
    if (!s || busy) return;
    clear(state);
    clear(action);
    const name = isolate(nameOf(s));
    if (s.auth.present) {
      state.append(statusBadge("ok", t("mcpc.signedIn")));
      action.append(h("button", { type: "button", class: "link", text: t("mcpc.signOut"), onclick: () => void signOut() }));
    } else {
      // Signed out is what keeps an enabled server from working: say it in the warn colour.
      state.append(statusBadge(s.enabled ? "warn" : "off", t("mcpc.signedOut")));
      const button = h("button", { type: "button", class: "primary sm", text: t("mcpc.signIn"), "aria-label": t("mcpc.signInNamed", { name }) }) as HTMLButtonElement;
      button.addEventListener("click", () => void signIn(button));
      action.append(button);
    }
  }

  const el = h("div", { class: "mcpc-auth" },
    h("span", { class: "mcpc-label", text: t("mcpc.oauthLabel") }),
    h("div", { class: "mcpc-auth-line" }, state, action),
    hint,
    failure,
  );
  return { el, sync };
}

/**
 * One secret (token or environment value): never shown back. Save while typed,
 * Remove (confirmed inline) when stored and the field is empty.
 */
function secretField(
  id: string, slot: McpcSecretSlot, label: string, present: () => boolean,
  opts: { required?: boolean; env?: string; hint?: string } = {},
): { el: HTMLElement; sync: () => void } {
  const required = opts.required ?? false;
  const fid = `mcpc-${domId(id)}-${domId(slot)}`;
  const input = h("input", {
    id: fid, type: "password", dir: "ltr", autocomplete: "off", spellcheck: "false",
    "aria-required": required ? "true" : undefined, "data-env": opts.env,
  }) as HTMLInputElement;
  const hint = opts.hint ? h("p", { class: "key-help-text mcpc-var", text: opts.hint }) : null;
  const badgeSlot = h("span", { class: "key-status" });
  const action = h("span", { class: "key-action" });
  const confirmStrip = h("div", { class: "key-confirm" });
  let confirming = false;
  let failed = false;
  const owner = () => isolate(nameOf(serverOf(id) ?? { name: id } as McpcServerView));

  const write = async (value: string) => {
    try {
      if (value) await BridgeMcpc.setSecret(id, slot, value);
      else await BridgeMcpc.clearSecret(id, slot);
      failed = false;
      input.value = "";
      announce(`${owner()} · ${label}: ${t(value ? "status.saved" : "status.removed")}`);
      void Bridge.log(`settings: mcp server ${id} ${slot.startsWith("env:") ? "env value" : "token"} ${value ? "saved" : "removed"}`);
    } catch (err) {
      failed = true;
      announce(`${owner()} · ${label}: ${t("status.failed")}`);
      void Bridge.log(`settings: mcp server ${id} secret write failed: ${String(err).split(/[|\n]/, 1)[0]}`);
    }
    confirming = false;
    await refreshList();
    sync();
    rows.get(id)?.sync();
    // The Save / Remove link that had focus is gone.
    input.focus({ preventScroll: true });
  };

  function sync() {
    const stored = present();
    clear(badgeSlot);
    badgeSlot.append(
      failed ? statusBadge("err", t("status.failed"))
        : stored ? statusBadge("ok", t("status.saved"))
        : required ? statusBadge("warn", t("mcpc.required"))
        : statusBadge("off", t("status.empty")),
    );
    input.placeholder = stored ? t("mcpc.stored") : t("mcpc.paste");
    clear(action);
    clear(confirmStrip);
    if (confirming) {
      const yes = h("button", { type: "button", class: "danger sm", text: t("common.remove") }) as HTMLButtonElement;
      yes.addEventListener("click", () => {
        yes.disabled = true;
        void write("");
      });
      const no = h("button", { type: "button", class: "sm", text: t("common.cancel") }) as HTMLButtonElement;
      no.addEventListener("click", () => {
        confirming = false;
        sync();
        input.focus({ preventScroll: true });
      });
      confirmStrip.append(h("span", { class: "confirm", role: "alert", text: t("mcpc.confirmRemoveKey") }), yes, no);
      queueMicrotask(() => no.focus({ preventScroll: true }));
      return;
    }
    if (input.value.trim()) {
      action.append(h("button", {
        type: "button", class: "link", text: t("common.save"),
        "aria-label": t("mcpc.saveNamed", { field: label, name: owner() }),
        onclick: () => void write(input.value.trim()),
      }));
    } else if (stored) {
      action.append(h("button", {
        type: "button", class: "link danger-link",
        "aria-label": t("mcpc.removeNamed", { field: label, name: owner() }),
        onclick: () => { confirming = true; sync(); },
      }, icon("trash", 14), h("span", { text: t("common.remove") })));
    }
  }

  input.addEventListener("input", () => {
    if (!confirming) sync();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && input.value.trim()) {
      e.preventDefault();
      void write(input.value.trim());
    }
  });
  sync();

  return {
    el: h("div", { class: "key-field" },
      h("div", { class: "key-label-row" }, h("label", { for: fid, text: label }), badgeSlot, h("span", { class: "spacer" }), action),
      input,
      hint,
      confirmStrip,
    ),
    sync,
  };
}

/** "Test connection": (re)connects, lists the tools, says what the server is. */
function testBlock(id: string): { el: HTMLElement; sync: () => void; run: () => Promise<void> } {
  const button = h("button", { type: "button", class: "sm", text: t("mcpc.test") }) as HTMLButtonElement;
  const result = h("span", { class: "int-test-result" });
  const info = h("span", { class: "int-updated", dir: "ltr" });
  const detail = h("p", { class: "int-test-err", dir: "auto" });
  const el = h("div", { class: "int-test mcpc-test" },
    h("div", { class: "int-test-line" }, button, result, h("span", { class: "spacer" }), info),
    detail,
  );
  let testing = false;

  const run = async () => {
    // aria-disabled rather than disabled, so keyboard focus stays on the button.
    if (testing) return;
    testing = true;
    button.setAttribute("aria-disabled", "true");
    el.setAttribute("aria-busy", "true");
    button.textContent = t("mcpc.testing");
    clear(result);
    detail.textContent = "";
    const name = isolate(nameOf(serverOf(id) ?? { name: id } as McpcServerView));
    try {
      const outcome = await BridgeMcpc.connect(id);
      toolLists.set(id, outcome.tools);
      result.append(statusBadge("ok", t("mcpc.testOk")));
      announce(`${name}: ${t("mcpc.testOk")} · ${toolCount(outcome.tools.length)}`);
    } catch (err) {
      result.append(statusBadge("err", t("mcpc.testFailed")));
      detail.textContent = errorText(err);
      announce(`${name}: ${detail.textContent}`);
    }
    testing = false;
    button.removeAttribute("aria-disabled");
    el.removeAttribute("aria-busy");
    button.textContent = t("mcpc.test");
    await refreshList();
    rows.get(id)?.sync();
  };
  button.addEventListener("click", () => void run());

  const sync = () => {
    const s = serverOf(id);
    if (!s) return;
    button.setAttribute("aria-label", t("mcpc.testNamed", { name: isolate(nameOf(s)) }));
    el.hidden = !canConnect(s);
    const sv = s.serverInfo;
    info.textContent = sv ? cleanText(`${sv.name} ${sv.version}`, 80) : "";
  };
  return { el, sync, run };
}

/** The server's tools, each with Auto / Ask / Off. */
function toolsBlock(id: string): { el: HTMLElement; sync: (open: boolean) => void } {
  const message = h("p", { class: "hint" });
  const search = h("input", {
    type: "search", class: "int-search", autocomplete: "off", spellcheck: "false",
    placeholder: t("mcpc.toolsSearch"), "aria-label": t("mcpc.toolsSearch"),
  }) as HTMLInputElement;
  followTextDirection(search);
  const list = h("ul", { class: "mcpc-tool-list" });
  const none = h("p", { class: "hint", dir: "auto" });
  const hint = h("p", { class: "key-help-text", text: t("mcpc.toolsHint") });
  const untrusted = h("p", { class: "storage-note", text: t("mcpc.toolsUntrusted") });
  const el = h("div", { class: "mcpc-tools" },
    h("div", { class: "mcpc-tools-head" }, h("h4", { text: t("mcpc.tools") })),
    message, hint, search, list, none, untrusted,
  );
  let drawn: McpcToolView[] | null = null;
  let fetching = false;
  const entries: { el: HTMLElement; words: string }[] = [];

  const filter = () => {
    const q = (toolQueries.get(id) ?? "").trim().toLowerCase();
    let shown = 0;
    for (const e of entries) {
      e.el.hidden = q !== "" && !e.words.includes(q);
      if (!e.el.hidden) shown++;
    }
    none.hidden = shown > 0 || entries.length === 0;
    none.textContent = none.hidden ? "" : t("mcpc.toolsNoMatch", { q: isolate(q) });
  };
  search.addEventListener("input", () => {
    toolQueries.set(id, search.value);
    filter();
  });
  search.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && search.value) {
      e.preventDefault();
      e.stopPropagation();
      search.value = "";
      search.dispatchEvent(new Event("input"));
    }
  });

  function drawList(listed: McpcToolView[]) {
    drawn = listed;
    clear(list);
    entries.length = 0;
    for (const tool of listed) {
      const row = toolRow(id, tool);
      list.append(row);
      entries.push({ el: row, words: `${tool.name} ${tool.title ?? ""} ${tool.description}`.toLowerCase() });
    }
    search.value = toolQueries.get(id) ?? "";
    filter();
  }

  async function fetchTools() {
    if (fetching) return;
    fetching = true;
    message.textContent = t("mcpc.toolsLoading");
    try {
      toolLists.set(id, await BridgeMcpc.tools(id));
    } catch (err) {
      message.textContent = errorText(err);
      fetching = false;
      return;
    }
    fetching = false;
    sync(true);
  }

  function sync(open: boolean) {
    const s = serverOf(id);
    if (!s) return;
    const listed = toolLists.get(id) ?? null;
    const show = (msg: string | null) => {
      message.textContent = msg ?? "";
      message.hidden = !msg;
      const has = !msg && !!listed && listed.length > 0;
      list.hidden = hint.hidden = untrusted.hidden = !has;
      search.hidden = !has || listed!.length <= TOOL_SEARCH_FROM;
      if (!has) none.hidden = true;
    };
    // Before approval or sign-in there is nothing to list, and the step above says what to do.
    el.hidden = !listed && s.enabled && !canConnect(s);
    if (!s.enabled) {
      show(t("mcpc.toolsOff"));
      return;
    }
    if (listed) {
      if (drawn !== listed) drawList(listed);
      show(listed.length ? null : t("mcpc.toolsEmpty"));
      return;
    }
    if (fetching) return;
    // Only an already-running server is asked; opening a row never starts one.
    if (open && s.status === "ready") {
      show(t("mcpc.toolsLoading"));
      void fetchTools();
      return;
    }
    show(t("mcpc.toolsNotLoaded"));
  }

  return { el, sync };
}

const MODES: McpcToolMode[] = ["auto", "ask", "off"];

function toolRow(id: string, tool: McpcToolView): HTMLElement {
  const toolName = cleanText(tool.name, 64);
  const title = tool.title ? cleanText(tool.title, 80) : "";
  const desc = cleanText(tool.description, 300);
  const group = h("div", { class: "segmented mcpc-modes", role: "radiogroup", "aria-label": t("mcpc.modeOf", { tool: isolate(toolName) }) });
  const failure = h("p", { class: "int-test-err mcpc-tool-err", role: "alert", dir: "auto" });
  const buttons = new Map<McpcToolMode, HTMLButtonElement>();

  const paint = () => {
    for (const [mode, b] of buttons) {
      const on = tool.mode === mode;
      b.classList.toggle("on", on);
      b.setAttribute("aria-checked", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
  };
  const choose = async (mode: McpcToolMode) => {
    if (tool.mode === mode) return;
    const before = tool.mode;
    tool.mode = mode;
    paint();
    failure.textContent = "";
    try {
      await BridgeMcpc.setToolMode(id, tool.name, mode);
    } catch (err) {
      tool.mode = before;
      paint();
      failure.textContent = t("mcpc.modeFailed", { tool: isolate(toolName), err: errorText(err) });
      return;
    }
    // "Off" changes how many tools the chat is offered.
    refreshSoon();
  };

  MODES.forEach((mode, i) => {
    const b = h("button", { type: "button", class: "seg", role: "radio", text: t(`mcpc.mode.${mode}`) }) as HTMLButtonElement;
    b.addEventListener("click", () => void choose(mode));
    b.addEventListener("keydown", (e) => {
      // The group reads in the UI's direction, so the arrows follow it.
      const ahead = e.key === "ArrowDown" || e.key === (isRtl() ? "ArrowLeft" : "ArrowRight");
      const behind = e.key === "ArrowUp" || e.key === (isRtl() ? "ArrowRight" : "ArrowLeft");
      if (!ahead && !behind) return;
      e.preventDefault();
      const next = MODES[(i + (ahead ? 1 : -1) + MODES.length) % MODES.length];
      buttons.get(next)?.focus();
      void choose(next);
    });
    buttons.set(mode, b);
    group.append(b);
  });
  paint();

  return h("li", { class: "mcpc-tool" },
    h("div", { class: "mcpc-tool-text" },
      h("div", { class: "mcpc-tool-top" },
        h("code", { class: "mcpc-tool-name", dir: "ltr", text: toolName }),
        title && title !== toolName ? h("span", { class: "mcpc-tool-title", dir: "auto", text: title }) : null,
        tool.readOnly ? h("span", { class: "mcpc-flag", text: t("mcpc.readOnly") }) : null,
        tool.destructive ? h("span", { class: "mcpc-flag warn", text: t("mcpc.destructive") }) : null,
      ),
      desc ? h("p", { class: "mcpc-tool-desc", dir: "auto", title: desc, text: desc }) : null,
    ),
    group,
    failure,
  );
}

/** "Remove server", confirmed inline. */
function removeFoot(id: string): HTMLElement {
  const confirmStrip = h("div", { class: "key-confirm" });
  const error = h("p", { class: "int-test-err", role: "alert", dir: "auto" });
  const remove = h("button", { type: "button", class: "link danger-link" },
    icon("trash", 14), h("span", { text: t("mcpc.remove") })) as HTMLButtonElement;
  const name = () => isolate(nameOf(serverOf(id) ?? { name: id } as McpcServerView));
  remove.setAttribute("aria-label", t("mcpc.removeServerNamed", { name: name() }));

  const back = () => {
    clear(confirmStrip);
    remove.hidden = false;
    remove.focus({ preventScroll: true });
  };
  remove.addEventListener("click", () => {
    error.textContent = "";
    remove.hidden = true;
    const yes = h("button", { type: "button", class: "danger sm", text: t("common.remove") }) as HTMLButtonElement;
    const no = h("button", { type: "button", class: "sm", text: t("common.cancel"), onclick: back }) as HTMLButtonElement;
    yes.addEventListener("click", async () => {
      yes.disabled = no.disabled = true;
      const label = name();
      try {
        await BridgeMcpc.remove(id);
      } catch (err) {
        back();
        error.textContent = t("mcpc.removeFailed", { name: label, err: errorText(err) });
        return;
      }
      void Bridge.log(`settings: mcp server removed ${id}`);
      expanded.delete(id);
      toolLists.delete(id);
      toolQueries.delete(id);
      rowNotices.delete(id);
      await refreshList();
      announce(t("mcpc.removed", { name: label }));
      document.querySelector<HTMLElement>("#mcpc-add:not([hidden]), #mcpc-add-empty")?.focus({ preventScroll: true });
    });
    confirmStrip.append(h("span", { class: "confirm", role: "alert", text: t("mcpc.confirmRemove", { name: name() }) }), yes, no);
    no.focus({ preventScroll: true });
  });

  return h("div", { class: "int-foot mcpc-foot" },
    h("div", { class: "int-foot-line" }, h("span", { class: "spacer" }), remove),
    confirmStrip,
    error,
  );
}
