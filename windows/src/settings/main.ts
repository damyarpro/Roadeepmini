// Settings window — the place where anything that writes to disk is confirmed.
// Roadeep account and agents (roadeep-section.ts), Claude Code hooks,
// integrations and the general preferences, behind a section navigation.

import "./settings.css";
import { Bridge, IS_TAURI, onEvent } from "../core/bridge";
import { DEFAULT_DOCK, DEFAULT_SETTINGS, MAX_ACTIVE_PILLS, type DockSettings, type Settings } from "../core/state";
import {
  getLanguage, isRtl, isolate, loadFonts, normalizeLanguage, setLanguage, t,
  type Language,
} from "../core/i18n";
import { h, clear } from "../views/dom";
import { accountSection, initRoadeepSections } from "./roadeep-section";
import { agentGroups, initAgentsSection } from "./agents-section";
import { dropPillsListeners, initPills, isPillOn, onPillsChange, pillSwitch, pillsUsed, refreshPills } from "./pills";
import { mcpSection } from "./mcp-section";
import { codingHooksSection } from "./coding-hooks-section";
import { mcpServersSection } from "./mcp-servers";
import { installContextMenu, quitEntry } from "../core/context-menu";
import { followTextDirection, helpDisclosure, icon, linkButton, sectionHead, settingRow, statusBadge, switchEl } from "./ui";
import { localizeError } from "../core/error-text";
import { BridgeInt, POLL_STATUS_EVENT, type PollStatus } from "../core/bridge-int";
import { timeAgo } from "../views/integrations";
import { updateRows } from "./update-row";
import {
  CATEGORIES, isCategory, isWebUrl, loadCatalog, localized, pillIdOf,
  type CatalogEntry, type Category, type Localized,
} from "../core/catalog";
import { openMarket, type MarketItem } from "./market";
import { LINE_DELAY_OPTIONS, lineDelaySeconds } from "../core/idle";
import { eyeMotionRow, plannerSection, refreshPlannerSection } from "./planner-section";
import { appearanceRows, refreshAppearance } from "./appearance";
import { normalizeAppearance } from "../character/appearance";
import { assistantSettingsSection } from "./assistant-settings";
import { settingsDashboard, settingsDestination, categoryAnchor as anchorId, type SettingsDestination } from "./dashboard";
import { initShortcutsSection, refreshShortcutsSection, shortcutsSection } from "./shortcuts-section";

const ROADEEP_SITE = "https://roadeep.com";

let settings: Settings = { ...DEFAULT_SETTINGS };
let version = "";

const root = document.getElementById("settings-root")!;

async function save() {
  await Bridge.saveSettings(settings);
}

/** Language, direction and title of the whole window. Paths and diffs opt back into LTR. */
function applyLanguage(lang: Language) {
  setLanguage(lang);
  document.documentElement.dir = isRtl() ? "rtl" : "ltr";
  document.title = t("settings.windowTitle");
}

// ── Integrations section ──────────────────────────────────────────────────────

/** A hand-coded service's text is an i18n key; a catalog service brings its own fa/en pair. */
type Text = string | Localized;
const txt = (text: Text) => (typeof text === "string" ? t(text) : localized(text));

interface IntegrationDef {
  id: string;
  name: string;
  color: string;
  /** Subhead label `integrations.cat.<category>`, in core/catalog.ts CATEGORIES order. */
  category: Category;
  /** The service's own page for creating a key (fixed https URL), when it has one. */
  keyUrl?: string;
  /**
   * Credential Manager keys, in the order they are shown. `label` is an i18n
   * key; `placeholder` is a format hint, or "" for the generic "paste here";
   * `help` (i18n key) says where to find the value. An `optional` field has a
   * default on the Rust side and never makes the service "need a key".
   */
  fields: { key: string; label: Text; placeholder: string; secret: boolean; help?: Text; optional?: boolean }[];
  /** Catalog services only: the manifest it was built from (hosts, docs, description). */
  entry?: CatalogEntry;
}

// Colours match INTEGRATION_AGENTS in core/state.ts (the island pills).
const INTEGRATIONS: IntegrationDef[] = [
  { id: "integration_stripe", name: "Stripe", color: "#0570DE", category: "payments", keyUrl: "https://dashboard.stripe.com/apikeys",
    fields: [{ key: "stripe-api-key", label: "integrations.secretKey", placeholder: "sk_live_…", secret: true }] },
  { id: "integration_github", name: "GitHub", color: "#F4505E", category: "dev", keyUrl: "https://github.com/settings/tokens",
    fields: [{ key: "github-token", label: "integrations.token", placeholder: "ghp_…", secret: true }] },
  { id: "integration_gitlab", name: "GitLab", color: "#FC6D26", category: "dev",
    keyUrl: "https://gitlab.com/-/user_settings/personal_access_tokens",
    fields: [
      { key: "gitlab-token", label: "integrations.accessToken", placeholder: "glpat-…", secret: true, help: "integrations.gitlabKeyHelp" },
      { key: "gitlab-url", label: "integrations.instanceUrl", placeholder: "https://gitlab.com", secret: false,
        help: "integrations.gitlabUrlHelp", optional: true },
    ] },
  { id: "integration_vercel", name: "Vercel", color: "#7C5CFF", category: "dev", keyUrl: "https://vercel.com/account/tokens",
    fields: [{ key: "vercel-token", label: "integrations.token", placeholder: "", secret: true }] },
  { id: "integration_netlify", name: "Netlify", color: "#32E6E2", category: "dev",
    keyUrl: "https://app.netlify.com/user/applications#personal-access-tokens",
    fields: [{ key: "netlify-token", label: "integrations.accessToken", placeholder: "nfp_…", secret: true }] },
  { id: "integration_cloudflare", name: "Cloudflare", color: "#FACC15", category: "dev",
    keyUrl: "https://dash.cloudflare.com/profile/api-tokens",
    fields: [
      { key: "cloudflare-token", label: "integrations.apiToken", placeholder: "", secret: true, help: "integrations.cloudflareKeyHelp" },
      { key: "cloudflare-account-id", label: "integrations.accountId", placeholder: "0123456789abcdef0123456789abcdef", secret: false,
        help: "integrations.cloudflareAccountHelp" },
    ] },
  { id: "integration_sentry", name: "Sentry", color: "#FF45A8", category: "monitoring",
    keyUrl: "https://sentry.io/settings/account/api/auth-tokens/",
    fields: [
      { key: "sentry-token", label: "integrations.authToken", placeholder: "sntryu_…", secret: true, help: "integrations.sentryKeyHelp" },
      { key: "sentry-org", label: "integrations.orgSlug", placeholder: "my-org", secret: false, help: "integrations.sentryOrgHelp", optional: true },
      { key: "sentry-url", label: "integrations.regionUrl", placeholder: "https://sentry.io", secret: false,
        help: "integrations.sentryUrlHelp", optional: true },
    ] },
  { id: "integration_linear", name: "Linear", color: "#5E6AD2", category: "work",
    keyUrl: "https://linear.app/settings/account/security",
    fields: [{ key: "linear-api-key", label: "integrations.apiKey", placeholder: "lin_api_…", secret: true, help: "integrations.linearKeyHelp" }] },
  { id: "integration_notion", name: "Notion", color: "#8C8C8C", category: "work", keyUrl: "https://www.notion.so/my-integrations",
    fields: [{ key: "notion-api-key", label: "integrations.integrationToken", placeholder: "ntn_…", secret: true }] },
  { id: "integration_calcom", name: "Cal.com", color: "#C9956A", category: "work", keyUrl: "https://app.cal.com/settings/developer/api-keys",
    fields: [{ key: "calcom-api-key", label: "integrations.apiKey", placeholder: "cal_…", secret: true }] },
  { id: "integration_resend", name: "Resend", color: "#22C55E", category: "comms", keyUrl: "https://resend.com/api-keys",
    fields: [{ key: "resend-api-key", label: "integrations.apiKey", placeholder: "re_…", secret: true }] },
  { id: "integration_n8n", name: "n8n", color: "#F29B38", category: "automation",
    fields: [
      { key: "n8n-url", label: "integrations.instanceUrl", placeholder: "https://n8n.example.com", secret: false },
      { key: "n8n-api-key", label: "integrations.apiKey", placeholder: "", secret: true, help: "integrations.n8nKeyHelp" },
    ] },
];

/** Catalog services (core/catalog.ts), once the catalog has been read. */
let catalogDefs: IntegrationDef[] = [];

/** Every service the market offers: the hand-coded ones, then the catalog's. */
const allDefs = () => [...INTEGRATIONS, ...catalogDefs];
const defOf = (id: string) => allDefs().find((d) => d.id === id) ?? null;
const isAdded = (id: string) => settings.addedIntegrations.includes(id);

/** A manifest as a service of this section; null when it can't be placed. */
function catalogDef(entry: CatalogEntry): IntegrationDef | null {
  const id = pillIdOf(entry.id);
  if (!isCategory(entry.category) || INTEGRATIONS.some((d) => d.id === id)) {
    void Bridge.log(`settings: catalog entry ${entry.id} skipped (category or id)`);
    return null;
  }
  return {
    id, name: entry.name, color: entry.color, category: entry.category,
    keyUrl: isWebUrl(entry.keyUrl) ? entry.keyUrl : undefined,
    fields: entry.fields.map((f) => ({
      key: f.key, label: f.label, placeholder: f.placeholder, secret: f.kind === "secret",
      help: f.help ?? undefined, optional: f.optional,
    })),
    entry,
  };
}

/** The one-line description the market shows. */
const descOf = (def: IntegrationDef) =>
  def.entry ? localized(def.entry.desc) : t(`market.desc.${def.id.slice("integration_".length)}`);

/** Reads which keys of these services are stored, and the values that may be shown back. */
async function loadKeys(defs: IntegrationDef[]) {
  for (const def of defs) {
    for (const f of def.fields) {
      presentKeys[f.key] = (await Bridge.secretPresent(f.key)) ?? false;
      if (!f.secret) publicValues[f.key] = (await Bridge.secretPublicValue(f.key)) ?? "";
    }
  }
}

/** The search box only earns its place once the list is long. */
const SEARCH_FROM = 8;

/** What each service's last poll said (integrations.rs), kept live by an event. */
const pollStatus: Record<string, PollStatus> = {};
/** Redraws a service's test row and badge; replaced on every render. */
const statusListeners = new Map<string, () => void>();

function setPollStatus(status: PollStatus) {
  pollStatus[status.id] = status;
  statusListeners.get(status.id)?.();
}

function keyBadge(state: "saved" | "empty" | "failed"): HTMLElement {
  return state === "saved"
    ? statusBadge("ok", t("status.saved"))
    : state === "failed"
      ? statusBadge("err", t("status.failed"))
      : statusBadge("off", t("status.empty"));
}

type FieldDef = IntegrationDef["fields"][number];

/** Stored values of the non-secret fields (n8n's URL), shown and edited as text. */
const publicValues: Record<string, string> = {};

const helpId = (key: string) => `int-help-${key}`;

/**
 * One credential. Label row: label, status, and at the inline end a quiet
 * action (Save while typed, Remove when stored and empty). The input below is
 * always full width; removing asks in a strip under it.
 */
function keyField(
  owner: string, field: FieldDef, present: Record<string, boolean>,
  announce: (s: string) => void, changed: () => void,
): HTMLElement {
  const id = `int-${field.key}`;
  const fieldName = txt(field.label);
  const stored = () => (field.secret ? "" : publicValues[field.key] ?? "");
  const input = h("input", {
    id,
    type: field.secret ? "password" : "text",
    dir: "ltr",
    autocomplete: "off",
    spellcheck: "false",
    "aria-describedby": field.help ? helpId(field.key) : undefined,
  }) as HTMLInputElement;
  input.value = stored();
  const setPlaceholder = () => {
    input.placeholder = field.secret && present[field.key]
      ? t("integrations.storedPlaceholder")
      : field.placeholder || t("integrations.pastePlaceholder");
  };
  setPlaceholder();

  const badgeSlot = h("span", { class: "key-status" }, keyBadge(present[field.key] ? "saved" : "empty"));
  const setBadge = (state: "saved" | "empty" | "failed") => {
    clear(badgeSlot);
    badgeSlot.append(keyBadge(state));
  };
  const action = h("span", { class: "key-action" });
  const confirmStrip = h("div", { class: "key-confirm" });
  const help = field.help ? h("p", { class: "key-help-text", id: helpId(field.key), text: txt(field.help) }) : null;
  let confirming = false;

  // Typed text → save; a stored value with nothing in its place → remove
  // (writing "" clears it, as it always did); otherwise there is nothing to do.
  const pending = (): "save" | "remove" | null => {
    const value = input.value.trim();
    if (field.secret) return value ? "save" : present[field.key] ? "remove" : null;
    if (value === stored()) return null;
    return value ? "save" : "remove";
  };

  const write = async (value: string) => {
    const prefix = `${owner} · ${fieldName}: `;
    try {
      await Bridge.secretSet(field.key, value);
      present[field.key] = value.length > 0;
      if (!field.secret) publicValues[field.key] = value;
      input.value = stored();
      setBadge(value ? "saved" : "empty");
      announce(prefix + t(value ? "status.saved" : "status.removed"));
    } catch {
      setBadge("failed");
      announce(prefix + t("status.failed"));
    }
    setPlaceholder();
    confirming = false;
    sync();
    changed();
  };

  function sync() {
    clear(action);
    clear(confirmStrip);
    if (help) help.hidden = present[field.key] === true;
    if (confirming) {
      const yes = h("button", { type: "button", class: "danger sm", text: t("common.remove") }) as HTMLButtonElement;
      yes.addEventListener("click", () => {
        yes.disabled = true;
        void write("");
      });
      const no = h("button", { type: "button", class: "sm", text: t("common.cancel") }) as HTMLButtonElement;
      no.addEventListener("click", () => {
        confirming = false;
        if (!field.secret) input.value = stored();
        sync();
        input.focus({ preventScroll: true });
      });
      confirmStrip.append(h("span", { class: "confirm", role: "alert", text: t("integrations.confirmRemove") }), yes, no);
      queueMicrotask(() => no.focus({ preventScroll: true }));
      return;
    }
    const what = pending();
    if (what === "save") {
      action.append(h("button", {
        type: "button", class: "link", text: t("common.save"),
        "aria-label": t("integrations.saveNamed", { field: fieldName, name: owner }),
        onclick: () => void write(input.value.trim()),
      }));
    } else if (what === "remove") {
      // Quiet at rest; the red button only appears in the confirm strip.
      action.append(h("button", {
        type: "button", class: "link danger-link",
        "aria-label": t("integrations.removeNamed", { field: fieldName, name: owner }),
        onclick: () => {
          confirming = true;
          sync();
        },
      }, icon("trash", 14), h("span", { text: t("common.remove") })));
    }
  }

  input.addEventListener("input", () => {
    if (!confirming) sync();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && pending() === "save") {
      e.preventDefault();
      void write(input.value.trim());
    }
  });
  sync();

  return h("div", { class: "key-field" },
    h("div", { class: "key-label-row" },
      h("label", { for: id, text: field.optional ? t("integrations.optional", { field: fieldName }) : fieldName }),
      badgeSlot,
      h("span", { class: "spacer" }),
      action,
    ),
    input,
    help,
    confirmStrip,
  );
}

/** Open/closed state of each service's key fields, kept across redraws. */
const expandedInts = new Map<string, boolean>();

/**
 * One service: [swatch · name · status · chevron] is the disclosure button for
 * its key fields; the island switch is the row's only strong control.
 */
function integrationItem(def: IntegrationDef, present: Record<string, boolean>, announce: (s: string) => void): HTMLElement {
  const required = def.fields.filter((f) => !f.optional);
  const anyStored = () => def.fields.some((f) => present[f.key]);
  const allStored = () => required.every((f) => present[f.key]);
  let expanded = expandedInts.get(def.id) ?? anyStored();

  const fieldsId = `int-fields-${def.id}`;
  const badgeSlot = h("span", { class: "int-badge" });
  const sw = pillSwitch(def.id, t("pills.showNamed", { name: def.name }));
  const rows = h("div", { class: "int-fields sub-strip", id: fieldsId });
  const disclose = h("button", {
    type: "button", class: "int-id", "aria-controls": fieldsId,
  },
    h("i", { class: "agent-swatch", style: `--c:${def.color}`, "aria-hidden": "true" }),
    h("span", { class: "int-name", text: def.name }),
    badgeSlot,
    h("span", { class: "chev", "aria-hidden": "true" }, icon("chevronDown", 16)),
  ) as HTMLButtonElement;

  const missingHelp = required.find((f) => f.help && !present[f.key]);
  const failing = () => allStored() && !!pollStatus[def.id]?.error;

  const refresh = () => {
    // A pill that is on but cannot work yet says so; it stays switchable,
    // since the key may be added afterwards.
    clear(badgeSlot);
    if (isPillOn(def.id) && !allStored()) {
      const warn = statusBadge("warn", t("integrations.keyNeeded"));
      if (missingHelp && !present[missingHelp.key]) warn.setAttribute("aria-describedby", helpId(missingHelp.key));
      badgeSlot.append(warn);
    } else if (!expanded) {
      badgeSlot.append(
        failing() ? statusBadge("err", t("integrations.connError"))
          : allStored() ? statusBadge("ok", t("status.saved"))
          : statusBadge("off", t("status.empty")),
      );
    }
    disclose.setAttribute("aria-expanded", expanded ? "true" : "false");
    disclose.title = t(expanded ? "integrations.hideKeys" : anyStored() ? "integrations.editKeys" : "integrations.addKey");
    rows.hidden = !expanded;
  };

  disclose.addEventListener("click", () => {
    expanded = !expanded;
    expandedInts.set(def.id, expanded);
    refresh();
    if (expanded) {
      rows.querySelector<HTMLInputElement>("input")?.focus({ preventScroll: true });
      // Only the content column scrolls (html/body are clipped).
      rows.scrollIntoView({ block: "nearest", behavior: "auto" });
    }
  });
  onPillsChange(`int-${def.id}`, refresh);

  const test = testRow(def, allStored, announce);
  statusListeners.set(def.id, () => {
    refresh();
    test.sync();
  });
  // A new key makes the last outcome meaningless (Rust forgets it too).
  const keyChanged = () => {
    delete pollStatus[def.id];
    refresh();
    test.sync();
  };

  for (const field of def.fields) rows.append(keyField(def.name, field, present, announce, keyChanged));
  if (def.entry) rows.append(hostsNote(def.entry));
  rows.append(test.el, itemFoot(def));
  refresh();
  test.sync();

  return h("div", { class: "int-item", id: `int-item-${def.id}` },
    h("div", { class: "int-head" }, disclose, h("div", { class: "int-ctrls" }, sw)),
    rows,
  );
}

/**
 * Where a catalog service's key may go, from its manifest: the engine refuses
 * any other host, so this is a promise the app keeps, not a hint.
 */
function hostsNote(entry: CatalogEntry): HTMLElement {
  const hosts = entry.hosts.map((host) => {
    const m = /^\{field\.([A-Za-z0-9_-]+)\}$/.exec(host);
    const field = m ? entry.fields.find((f) => f.name === m[1]) : undefined;
    return field ? t("market.hostField", { field: localized(field.label) }) : isolate(host);
  });
  return h("p", { class: "key-help-text int-hosts" },
    icon("lock", 14),
    h("span", { text: t("market.hosts", { hosts: hosts.join(isRtl() ? "، " : ", ") }) }),
  );
}

/**
 * Clears every stored key of a service, then un-adds it and turns its pill off.
 * A key that can't be cleared keeps the service listed, so it can be retried.
 */
async function removeService(def: IntegrationDef): Promise<boolean> {
  let failed = false;
  for (const f of def.fields) {
    // Asked afresh: a stale "absent" here would leave a key behind.
    if (!((await Bridge.secretPresent(f.key)) ?? false)) continue;
    try {
      await Bridge.secretClear(f.key);
    } catch (err) {
      failed = true;
      void Bridge.log(`settings: removing ${def.id}: clearing ${f.key} failed: ${String(err)}`);
    }
  }
  if (failed) return false;
  for (const f of def.fields) {
    presentKeys[f.key] = false;
    if (!f.secret) publicValues[f.key] = "";
  }
  settings.activeIntegrations = settings.activeIntegrations.filter((x) => x !== def.id);
  settings.addedIntegrations = settings.addedIntegrations.filter((x) => x !== def.id);
  delete pollStatus[def.id];
  expandedInts.delete(def.id);
  await save();
  refreshPills();
  void Bridge.log(`settings: service removed ${def.id}`);
  return true;
}

/** Under a service's keys: where to get one, its API docs, and Remove (confirmed inline). */
function itemFoot(def: IntegrationDef): HTMLElement {
  const links = h("div", { class: "key-help int-links" });
  if (def.keyUrl) {
    const url = def.keyUrl;
    links.append(linkButton(t("integrations.getKey"), () => void Bridge.openUrl(url)));
  }
  const docs = def.entry?.docsUrl;
  if (isWebUrl(docs)) links.append(linkButton(t("market.docs"), () => void Bridge.openUrl(docs)));

  const confirmStrip = h("div", { class: "key-confirm" });
  const error = h("p", { class: "int-test-err", role: "alert", dir: "auto" });
  // Quiet at rest, like a key's Remove; the red button only appears in the confirm strip.
  const remove = h("button", {
    type: "button", class: "link danger-link", "aria-label": t("market.removeNamed", { name: def.name }),
  }, icon("trash", 14), h("span", { text: t("market.remove") })) as HTMLButtonElement;

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
      if (await removeService(def)) {
        redrawIntegrations();
        announceInt(t("market.removed", { name: def.name }));
        document.querySelector<HTMLElement>("#int-add:not([hidden]), #int-add-empty")?.focus({ preventScroll: true });
        return;
      }
      back();
      error.textContent = t("market.removeFailed", { name: def.name });
      announceInt(error.textContent);
    });
    confirmStrip.append(
      h("span", { class: "confirm", role: "alert", text: t("market.confirmRemove", { name: def.name }) }), yes, no,
    );
    no.focus({ preventScroll: true });
  });

  return h("div", { class: "int-foot" },
    h("div", { class: "int-foot-line" }, links, h("span", { class: "spacer" }), remove),
    confirmStrip,
    error,
  );
}

/** The shared "{used}/{max}" chip: integrations and agents fill the same pill slots. */
function pillCounter(key: string): HTMLElement {
  const counter = h("span", { class: "chip counter-chip", role: "status" });
  const update = () => {
    const used = pillsUsed();
    counter.textContent = t("integrations.counter", { used, max: MAX_ACTIVE_PILLS });
    counter.setAttribute("aria-label", t("integrations.counterLabel", { used, max: MAX_ACTIVE_PILLS }));
    counter.title = t("integrations.counterLabel", { used, max: MAX_ACTIVE_PILLS });
    counter.classList.toggle("full", used >= MAX_ACTIVE_PILLS);
  };
  onPillsChange(key, update);
  update();
  return counter;
}

/** "Updated 3 min ago"; "just now" already says when. */
function updatedText(ms: number): string {
  const ago = timeAgo(ms);
  return t("integrations.updated", { t: ago === t("time.justNow") ? ago : t("time.ago", { t: ago }) });
}

/**
 * Under a configured service's keys: Test connection, the outcome of the last
 * poll (a test or the timer, whichever came last) and when it last answered.
 */
function testRow(
  def: IntegrationDef, configured: () => boolean, announce: (s: string) => void,
): { el: HTMLElement; sync: () => void } {
  const button = h("button", {
    type: "button", class: "sm", text: t("integrations.test"),
    "aria-label": t("integrations.testNamed", { name: def.name }),
  }) as HTMLButtonElement;
  const result = h("span", { class: "int-test-result" });
  const updated = h("span", { class: "int-updated" });
  const detail = h("p", { class: "int-test-err", dir: "auto" });
  const el = h("div", { class: "int-test" },
    h("div", { class: "int-test-line" }, button, result, h("span", { class: "spacer" }), updated),
    detail,
  );
  let testing = false;

  const sync = () => {
    el.hidden = !configured();
    const status = pollStatus[def.id];
    updated.textContent = status?.lastOk ? updatedText(status.lastOk) : "";
    if (testing) return;
    clear(result);
    detail.textContent = status?.error ? localizeError(status.error) : "";
    if (status?.error) result.append(statusBadge("err", t("integrations.connError")));
    else if (status?.lastOk) result.append(statusBadge("ok", t("integrations.testOk")));
  };

  button.addEventListener("click", async () => {
    // aria-disabled rather than disabled, so keyboard focus stays on the button.
    if (testing) return;
    testing = true;
    button.setAttribute("aria-disabled", "true");
    el.setAttribute("aria-busy", "true");
    button.textContent = t("integrations.testing");
    clear(result);
    detail.textContent = "";
    const outcome = await BridgeInt.test(def.id);
    testing = false;
    button.removeAttribute("aria-disabled");
    el.removeAttribute("aria-busy");
    button.textContent = t("integrations.test");
    if (outcome?.ok) {
      setPollStatus(outcome.status);
    } else if (outcome) {
      pollStatus[def.id] = { id: def.id, lastOk: pollStatus[def.id]?.lastOk ?? null, error: outcome.error };
      statusListeners.get(def.id)?.();
    } else {
      sync();
    }
    if (outcome) announce(`${def.name}: ${outcome.ok ? t("integrations.testOk") : localizeError(outcome.error)}`);
  });

  return { el, sync };
}

/** The search text, kept across redraws (a language change). */
let intQuery = "";

/** One polite live region for the section, kept across redraws so a message survives one. */
const intLive = h("span", { class: "sr-only", role: "status", "aria-live": "polite" });
function announceInt(text: string) {
  intLive.textContent = "";
  window.setTimeout(() => { intLive.textContent = text; }, 50);
}

/** The market lists every service; adding one opens it under "My services". */
function showMarket() {
  const items: MarketItem[] = allDefs().map((def) => ({
    id: def.id, name: def.name, color: def.color, category: def.category, desc: descOf(def),
  }));
  openMarket({ items, isAdded, onAdd: (id) => void addService(id) });
}

async function addService(id: string) {
  const def = defOf(id);
  if (!def) return;
  if (!isAdded(id)) settings.addedIntegrations = [...settings.addedIntegrations, id];
  await loadKeys([def]);
  expandedInts.set(id, true);
  // A search still typed would hide the newcomer.
  intQuery = "";
  await save();
  void Bridge.log(`settings: service added ${id}`);
  redrawIntegrations();
  const item = document.getElementById(`int-item-${id}`);
  item?.scrollIntoView({ block: "center", behavior: reduceMotion() ? "auto" : "smooth" });
  (item?.querySelector<HTMLElement>(".int-fields input") ?? item?.querySelector<HTMLElement>("button.int-id"))
    ?.focus({ preventScroll: true });
  announceInt(t("market.addedAnnounce", { name: def.name }));
}

/** Rebuilds just this section (a service added or removed), keeping the page's scroll. */
function redrawIntegrations() {
  const wrap = document.getElementById(anchorId("integrations"));
  if (!wrap) return;
  clear(wrap);
  wrap.append(integrationsSection(presentKeys));
}

/**
 * "Integrations": the services the user added ("My services"), under small
 * category subheads with a name filter once the list is long, and the way into
 * the market for the rest.
 */
function integrationsSection(present: Record<string, boolean>): HTMLElement {
  // Listeners of the previous drawing would keep redrawing detached rows.
  statusListeners.clear();
  dropPillsListeners("int-integration_");
  const counter = pillCounter("note");
  const groups = h("div", { class: "int-groups" });
  const announce = announceInt;
  const added = allDefs().filter((d) => isAdded(d.id));

  const entries: { el: HTMLElement; name: string; group: HTMLElement }[] = [];
  for (const cat of CATEGORIES) {
    const defs = added.filter((d) => d.category === cat);
    if (defs.length === 0) continue;
    const headId = `int-cat-${cat}`;
    const list = h("div", { class: "int-list" });
    const group = h("div", { class: "int-cat", role: "group", "aria-labelledby": headId },
      h("h4", { class: "int-cat-title", id: headId, text: t(`integrations.cat.${cat}`) }),
      list,
    );
    for (const def of defs) {
      const el = integrationItem(def, present, announce);
      list.append(el);
      entries.push({ el, name: def.name.toLowerCase(), group });
    }
    groups.append(group);
  }

  const empty = h("p", { class: "hint int-search-empty", dir: "auto" });
  let countTimer = 0;
  const applyFilter = (announceCount: boolean) => {
    const query = intQuery.trim().toLowerCase();
    let shown = 0;
    for (const entry of entries) {
      entry.el.hidden = query !== "" && !entry.name.includes(query);
      if (!entry.el.hidden) shown++;
    }
    for (const group of new Set(entries.map((e) => e.group))) {
      group.hidden = entries.every((e) => e.group !== group || e.el.hidden);
    }
    empty.hidden = shown > 0 || entries.length === 0;
    empty.textContent = empty.hidden ? "" : t("integrations.searchEmpty", { q: isolate(intQuery.trim()) });
    window.clearTimeout(countTimer);
    // Announced once typing pauses, not on every key.
    if (announceCount && query) {
      countTimer = window.setTimeout(() => announce(t("integrations.searchResults", { n: shown })), 500);
    }
  };

  let search: HTMLElement | null = null;
  if (added.length > SEARCH_FROM) {
    const input = h("input", {
      type: "search", class: "int-search", autocomplete: "off", spellcheck: "false",
      placeholder: t("integrations.search"), "aria-label": t("integrations.search"),
    }) as HTMLInputElement;
    input.value = intQuery;
    followTextDirection(input);
    input.addEventListener("input", () => {
      intQuery = input.value;
      applyFilter(true);
    });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && input.value) {
        e.preventDefault();
        input.value = "";
        input.dispatchEvent(new Event("input"));
      }
    });
    search = input;
  } else {
    intQuery = "";
  }
  applyFilter(false);

  const nothing = added.length === 0;
  const addButton = h("button", {
    type: "button", class: "sm int-add", id: "int-add", onclick: showMarket,
  }, icon("plus", 14), h("span", { text: t("market.open") })) as HTMLButtonElement;
  addButton.hidden = nothing;
  // Nothing added yet: the market is the only thing to do here.
  // Laid out like the agents' empty card, with four service colours.
  const emptyState = nothing
    ? h("div", { class: "empty-card" },
        h("div", { class: "empty-art", "aria-hidden": "true" },
          ...INTEGRATIONS.slice(0, 4).map((d) => h("i", { class: "agent-swatch", style: `--c:${d.color}` }))),
        h("div", { class: "agent-text" },
          h("div", { class: "agent-name", text: t("market.emptyTitle") }),
          h("div", { class: "hint", text: t("market.empty") }),
        ),
        h("button", { type: "button", class: "primary sm", id: "int-add-empty", onclick: showMarket },
          icon("plus", 16), h("span", { text: t("market.open") })),
      )
    : null;

  return h("section", { class: "sec int-agents", "aria-labelledby": "sec-int-title" },
    sectionHead({ id: "sec-int-title", icon: "integrations", title: t("integrations.title"), desc: t("section.integrationsDesc") }),
    h("div", { class: "group card", role: "group", "aria-labelledby": "grp-int" },
      h("div", { class: "subhead" },
        h("h3", { id: "grp-int", text: t("market.myServices") }), counter, h("span", { class: "spacer" }), addButton),
      nothing ? null : h("p", { class: "hint", text: t("integrations.switchHint", { max: MAX_ACTIVE_PILLS }) }),
      search,
      groups,
      empty,
      emptyState,
      h("p", { class: "storage-note", text: t("integrations.storageNote") }),
      intLive,
    ),
  );
}

/** "Agents": my agents and Roadeep's (agents-section.ts); they share the pill slots above. */
function agentsSection(): HTMLElement {
  return h("section", { class: "sec int-agents", "aria-labelledby": "sec-agents-title" },
    sectionHead({
      id: "sec-agents-title", icon: "agents", title: t("agents.title"), desc: t("section.agentsDesc"),
      badge: pillCounter("note-agents"),
    }),
    agentGroups(),
  );
}

// ── General section ───────────────────────────────────────────────────────────

/** Language names are written in their own language, so either can be found. */
const LANGUAGES: [Language, string][] = [
  ["fa", "فارسی"],
  ["en", "English"],
];

/** Redraws the position row from `settings.dock` (a drag on the island changes it). */
let refreshPosition: (() => void) | null = null;

/**
 * Where the island is docked. The edge buttons are the keyboard way to do what
 * a drag does; Rust moves the island and sends the new dock back.
 */
function positionRow(): HTMLElement {
  // Laid out like the screen, whatever the language: left, top, right.
  const edges: [DockSettings["edge"], string][] = [
    ["left", t("general.edgeLeft")],
    ["top", t("general.edgeTop")],
    ["right", t("general.edgeRight")],
  ];
  const reduced = () => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const dockTo = async (edge: DockSettings["edge"], reset: boolean) => {
    const ok = await Bridge.dockSet(edge, 0.5, reset, reduced());
    if (ok === false) void Bridge.log("settings: dock change refused (the island is moving)");
  };

  const group = h("div", { class: "segmented", role: "radiogroup", dir: "ltr", "aria-labelledby": "gen-position-edges" });
  const buttons = new Map<DockSettings["edge"], HTMLButtonElement>();
  edges.forEach(([edge, label], i) => {
    const btn = h("button", { type: "button", role: "radio", class: "seg", id: `gen-edge-${edge}`, text: label }) as HTMLButtonElement;
    btn.addEventListener("click", () => {
      if ((settings.dock ?? DEFAULT_DOCK).edge !== edge) void dockTo(edge, false);
    });
    btn.addEventListener("keydown", (e) => {
      // The group runs left to right in both languages, so the arrows do too.
      const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
      if (!step) return;
      e.preventDefault();
      const [next] = edges[(i + step + edges.length) % edges.length];
      buttons.get(next)?.focus();
      void dockTo(next, false);
    });
    buttons.set(edge, btn);
    group.append(btn);
  });
  const reset = linkButton(t("general.positionReset"), () => void dockTo("top", true));

  const sync = () => {
    const d = settings.dock ?? DEFAULT_DOCK;
    for (const [edge, btn] of buttons) {
      const on = d.edge === edge;
      btn.classList.toggle("on", on);
      btn.setAttribute("aria-checked", on ? "true" : "false");
      btn.tabIndex = on ? 0 : -1;
    }
    reset.hidden = d.edge === DEFAULT_DOCK.edge && Math.abs(d.pos - DEFAULT_DOCK.pos) < 0.001 && !d.monitor;
  };
  sync();
  refreshPosition = sync;

  return h("div", { class: "set-row stack" },
    h("div", { class: "set-text" },
      h("div", { class: "set-label-line" }, h("span", { class: "set-label", text: t("general.position") }), helpDisclosure(t("general.positionHint"), t("general.position"))),
    ),
    h("span", { class: "sr-only", id: "gen-position-edges", text: t("general.positionEdges") }),
    group,
    h("div", { class: "position-reset" }, reset),
  );
}

/** A delay for the idle-line select: off, or whole (or fractional) minutes. */
function lineDelayLabel(seconds: number): string {
  if (seconds === 0) return t("idle.off");
  const n = Math.round((seconds / 60) * 10) / 10;
  return n === 1 ? t("idle.oneMinute") : t("idle.minutes", { n });
}

/** How long the untouched island waits before it becomes a line (core/idle.ts). */
function idleLineRow(): HTMLElement {
  const current = lineDelaySeconds(settings.absenceInterval);
  // A value set by hand outside the choices is shown as it is, not lost.
  const values = LINE_DELAY_OPTIONS.includes(current)
    ? LINE_DELAY_OPTIONS
    : [...LINE_DELAY_OPTIONS, current].sort((a, b) => a - b);
  const select = h("select", { id: "gen-idle-line" }) as HTMLSelectElement;
  for (const v of values) select.append(h("option", { value: String(v), text: lineDelayLabel(v) }));
  select.value = String(current);
  select.addEventListener("change", () => {
    settings.absenceInterval = lineDelaySeconds(Number(select.value));
    void save();
  });
  return settingRow({ label: t("idle.line"), hint: t("idle.lineHint"), forId: "gen-idle-line" }, select);
}

function generalSection(): HTMLElement {
  const volume = h("input", {
    type: "range", min: "0", max: "0.2", step: "0.005",
    value: String(settings.soundVolume),
    "aria-label": t("general.sound"),
  }) as HTMLInputElement;
  volume.addEventListener("input", () => {
    settings.soundVolume = Number(volume.value);
    void save();
  });

  const autoClose = h("input", {
    id: "gen-autoclose",
    type: "number", min: "2", max: "120", step: "1",
    value: String(Math.round(settings.autoCloseInterval)),
    class: "num",
  }) as HTMLInputElement;
  autoClose.addEventListener("change", () => {
    settings.autoCloseInterval = Math.max(2, Math.min(120, Number(autoClose.value) || 15));
    autoClose.value = String(settings.autoCloseInterval);
    void save();
  });

  const screen = h("select", { id: "gen-screen" }) as HTMLSelectElement;
  screen.append(
    h("option", { value: "primary", text: t("general.screenPrimary") }),
    h("option", { value: "cursor", text: t("general.screenCursor") }),
  );
  screen.value = settings.screen;
  screen.addEventListener("change", () => {
    settings.screen = screen.value as Settings["screen"];
    void save();
  });

  const language = h("select", { id: "gen-language" }) as HTMLSelectElement;
  for (const [id, label] of LANGUAGES) {
    language.append(h("option", { value: id, lang: id, text: label }));
  }
  language.value = getLanguage();
  language.addEventListener("change", () => {
    // Applied here first so this window turns over at once; the save then
    // reaches the island through "settings-changed".
    settings.language = normalizeLanguage(language.value);
    applyLanguage(settings.language);
    render();
    void save();
  });

  return h(
    "section",
    { class: "sec", "aria-labelledby": "sec-general-title" },
    sectionHead({ id: "sec-general-title", icon: "general", title: t("general.title"), desc: t("section.generalDesc") }),
    h("div", { class: "card list" },
      settingRow({ label: t("general.language"), forId: "gen-language" }, language),
      settingRow({ label: t("general.sound") },
        volume,
        switchEl(settings.soundEnabled, false, t("general.sound"), (v) => { settings.soundEnabled = v; void save(); }),
      ),
      settingRow({ label: t("general.autoClose"), forId: "gen-autoclose" },
        autoClose,
        h("span", { class: "unit", text: t("general.autoCloseHint") }),
      ),
      idleLineRow(),
      settingRow({ label: t("general.screen"), forId: "gen-screen" }, screen),
      positionRow(),
      eyeMotionRow({ settings: () => settings, save }),
      appearanceRows({ settings: () => settings, save: () => IS_TAURI ? Bridge.saveSettingsChecked(settings) : Promise.resolve() }),
      settingRow({ label: t("general.autostart") },
        switchEl(settings.autostart, false, t("general.autostart"), (v) => { settings.autostart = v; void save(); }),
      ),
      ...updateRows({ settings: () => settings, save, version: () => version }),
    ),
  );
}

// ── Navigation ────────────────────────────────────────────────────────────────

let activeDestination: SettingsDestination = settingsDestination(new URLSearchParams(location.search).get("section"));
let dashboard: ReturnType<typeof settingsDashboard> | null = null;
const reduceMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ── Boot ──────────────────────────────────────────────────────────────────────

/** What the sections were built from, kept so a language change can redraw them. */
const presentKeys: Record<string, boolean> = {};

function render() {
  // Navigation retains these nodes; only an explicit language redraw rebuilds them.
  const focusedId = (document.activeElement as HTMLElement | null)?.id;
  clear(root);
  dashboard = settingsDashboard({
    initial: activeDestination,
    version,
    footer: h("footer", { class: "settings-overview-footer" }, h("span", { text: t("settings.footer") }), linkButton("roadeep.com", () => void Bridge.openUrl(ROADEEP_SITE))),
    onNavigate: destination => {
      activeDestination = destination;
      const url = new URL(location.href);
      if (destination === "dashboard") url.searchParams.delete("section");
      else url.searchParams.set("section", destination);
      history.replaceState(null, "", url);
    },
    panes: {
      account: h("div", { class: "sec-wrap", id: anchorId("account") }, accountSection()),
      integrations: h("div", { class: "sec-wrap", id: anchorId("integrations") }, integrationsSection(presentKeys)),
      agents: h("div", { class: "sec-wrap", id: anchorId("agents") }, agentsSection()),
      planner: h("div", { class: "sec-wrap", id: anchorId("planner") }, plannerSection({ settings: () => settings, save })),
      mcp: h("div", { class: "sec-wrap sec-stack", id: anchorId("mcp") }, codingHooksSection(), mcpSection()),
      mcpServers: h("div", { class: "sec-wrap", id: anchorId("mcpServers") }, mcpServersSection({ settings: () => settings, save })),
      assistant: h("div", { class: "sec-wrap", id: anchorId("assistant") }, assistantSettingsSection(undefined,{settings:()=>settings})),
      general: h("div", { class: "sec-wrap", id: anchorId("general") }, generalSection()),
      shortcuts: h("div", { class: "sec-wrap", id: anchorId("shortcuts") }, shortcutsSection({ settings: () => settings, save })),
    },
  });
  root.append(dashboard.element);
  if (focusedId) root.querySelector<HTMLElement>(`[id="${CSS.escape(focusedId)}"]`)?.focus({ preventScroll: true });
}

async function main() {
  // Settings would only reopen this window; closing it needs a window
  // permission the app doesn't grant (and the title bar already does it).
  installContextMenu({ entries: () => [quitEntry()] });
  const boot = await Bridge.boot();
  if (boot) {
    settings = { ...settings, ...boot.settings };
    version = boot.version;
  }
  settings.language = normalizeLanguage(settings.language);
  if (!IS_TAURI) {
    const params = new URLSearchParams(location.search);
    settings.language = normalizeLanguage(params.get("lang") ?? settings.language);
    if (["shape", "color", "expression", "body", "eyes"].some(key => params.has(key))) settings.characterAppearance = normalizeAppearance({shape:params.get("shape"),color:params.get("color"),expression:params.get("expression"),body:params.get("body"),eyes:params.get("eyes")});
  }
  applyLanguage(settings.language);
  await Promise.race([loadFonts(), new Promise<void>((resolve) => window.setTimeout(resolve, 800))]);

  // Account and agents draw themselves as their data arrives.
  const host = { settings: () => settings, save };
  initPills(host);
  initRoadeepSections(host);
  initAgentsSection(host);
  initShortcutsSection();

  catalogDefs = (await loadCatalog()).map(catalogDef).filter((d): d is IntegrationDef => d !== null);
  // Every hand-coded service (as before), but only the catalog services in use.
  await loadKeys([...INTEGRATIONS, ...catalogDefs.filter((d) => isAdded(d.id))]);
  for (const status of await BridgeInt.statuses()) pollStatus[status.id] = status;

  render();


  // Every poll reports here, so the outcome and "updated …" stay current.
  void onEvent<PollStatus>(POLL_STATUS_EVENT, setPollStatus);
  window.setInterval(() => {
    if (document.hidden) return;
    for (const redraw of statusListeners.values()) redraw();
  }, 30_000);

  // The island can open this window on a given section (e.g. "Sign in to Roadeep").
  void onEvent<string>("settings-focus", (requested) => {
    const destination = settingsDestination(requested);
    if (destination === "dashboard" && requested !== "dashboard") return;
    dashboard?.navigate(destination);
  });

  void onEvent<Settings>("settings-changed", (s) => {
    const addedBefore = settings.addedIntegrations.join();
    settings = { ...settings, ...s };
    settings.language = normalizeLanguage(settings.language);
    if (settings.language !== getLanguage()) {
      applyLanguage(settings.language);
      render();
    } else {
      refreshPills();
      refreshPosition?.();
      refreshPlannerSection();
      refreshAppearance();
      refreshShortcutsSection();
      // Rust drops ids it doesn't know; the list follows what it kept.
      if (settings.addedIntegrations.join() !== addedBefore) {
        void loadKeys(allDefs().filter((d) => isAdded(d.id))).then(redrawIntegrations);
      }
    }
  });
}

void main();
