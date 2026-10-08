// "Add an MCP server": a dialog over the settings window (built like the
// service market in market.ts) with the bundled directory of known servers,
// a short form for the ones that need a value first (a folder, a key), and a
// "custom server" form. Adding never runs anything: a local command still has
// to be approved under the server (mcp-servers.ts).

import { getLanguage, isolate, t } from "../core/i18n";
import { Bridge } from "../core/bridge";
import { localizeError } from "../core/error-text";
import {
  BridgeMcpc, MCPC_CATEGORIES,
  type McpcAddSpec, type McpcAuthSpec, type McpcAuthType, type McpcCategory, type McpcDirectoryEntry,
  type McpcLocalized, type McpcSecretSlot,
} from "../core/bridge-mcpc";
import { h, clear } from "../views/dom";
import { followTextDirection, icon, statusBadge } from "./ui";

// ── Pure helpers (also used by mcp-servers.ts and the tests) ─────────────────

const CONTROL = /[\u0000-\u001F\u007F-\u009F]/g;
// Bidi overrides, embeddings, isolates and marks: server text must not reorder ours.
const BIDI = /[؜‎‏‪-‮⁦-⁩]/g;

/**
 * Text that came from a server (names, tool descriptions) as one plain line:
 * no control or bidi-control characters, at most `max` characters. It is only
 * ever set as textContent; this keeps it from rearranging the line around it.
 */
export function cleanText(text: unknown, max: number): string {
  const line = String(text ?? "").replace(CONTROL, " ").replace(BIDI, "").replace(/\s+/g, " ").trim();
  const chars = Array.from(line);
  return chars.length > max ? `${chars.slice(0, Math.max(1, max - 1)).join("")}…` : line;
}

/** One argument as it would be typed: quoted only when it has to be. */
function quoteArg(arg: string): string {
  return arg === "" || /[\s"]/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg;
}

/** The command and its arguments on one line, so what is shown is what runs. */
export function commandLine(command: string, args: string[]): string {
  return [command, ...args].map(quoteArg).join(" ");
}

/** https anywhere; plain http only on this PC. No user:password@ in the URL. */
export function isServerUrl(raw: string): boolean {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return false;
  }
  if (url.username || url.password || !url.hostname) return false;
  if (url.protocol === "https:") return true;
  return url.protocol === "http:" && (url.hostname === "localhost" || url.hostname === "127.0.0.1");
}

/** "api.githubcopilot.com" for a row's second line; the raw text if it isn't a URL. */
export function urlHost(raw: string): string {
  try {
    return new URL(raw).host || raw;
  } catch {
    return raw;
  }
}

const MAX_ARGS = 40;
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;
const HEADER_NAME = /^[A-Za-z0-9-]{1,64}$/;
const LINE_BREAK = /[\r\n\0]/;

const lines = (text: string) => text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l !== "");

export interface CustomForm {
  name: string;
  kind: "http" | "stdio";
  url: string;
  command: string;
  /** One argument per line. */
  args: string;
  /** One variable name per line. */
  env: string;
  auth: McpcAuthType;
  header: string;
}

export type CustomField = "name" | "url" | "command" | "args" | "env" | "header";

/** The spec mcpc_add takes, or what is wrong with the form (Rust checks it all again). */
export function customSpec(form: CustomForm): { spec: McpcAddSpec | null; errors: Partial<Record<CustomField, string>> } {
  const errors: Partial<Record<CustomField, string>> = {};
  const name = form.name.trim();
  if (!name) errors.name = t("mcpc.err.required");
  else if (Array.from(name).length > 60) errors.name = t("mcpc.err.nameLong");
  else if (LINE_BREAK.test(name)) errors.name = t("mcpc.err.lineBreak");

  let spec: McpcAddSpec | null = null;
  if (form.kind === "http") {
    const url = form.url.trim();
    if (!url) errors.url = t("mcpc.err.required");
    else if (!isServerUrl(url)) errors.url = t("mcpc.err.url");
    let auth: McpcAuthSpec;
    if (form.auth === "header") {
      const header = form.header.trim();
      if (!header) errors.header = t("mcpc.err.required");
      else if (!HEADER_NAME.test(header)) errors.header = t("mcpc.err.header");
      auth = { type: "header", name: header };
    } else {
      auth = { type: form.auth };
    }
    spec = { name, source: "custom", transport: { type: "http", url }, auth };
  } else {
    const command = form.command.trim();
    if (!command) errors.command = t("mcpc.err.required");
    else if (LINE_BREAK.test(command)) errors.command = t("mcpc.err.command");
    const args = lines(form.args);
    if (args.length > MAX_ARGS) errors.args = t("mcpc.err.args");
    const env = [...new Set(lines(form.env))];
    const bad = env.find((n) => !ENV_NAME.test(n));
    if (bad) errors.env = t("mcpc.err.env", { name: isolate(bad) });
    // Its environment values are entered under the server; until then it stays off.
    spec = { name, source: "custom", transport: { type: "stdio", command, args, env }, auth: { type: "none" }, enabled: env.length === 0 };
  }
  return Object.keys(errors).length ? { spec: null, errors } : { spec, errors };
}

/** The spec for a directory entry, with the values typed for its argument fields. */
export function directorySpec(entry: McpcDirectoryEntry, argValues: Record<number, string>): McpcAddSpec {
  const auth: McpcAuthSpec = typeof entry.auth === "object"
    ? { type: "header", name: entry.auth.header }
    : { type: entry.auth };
  if (entry.transport.type === "http") {
    return { name: entry.name, source: `directory:${entry.id}`, transport: { type: "http", url: entry.transport.url }, auth };
  }
  const args = [...entry.transport.args];
  // The directory appends them (index = args.length + i); an index inside args replaces a placeholder.
  const fields = [...(entry.transport.argFields ?? [])].sort((a, b) => a.index - b.index);
  for (const field of fields) {
    const value = (argValues[field.index] ?? "").trim();
    if (field.index < args.length) args[field.index] = value;
    else args.push(value);
  }
  return {
    name: entry.name, source: `directory:${entry.id}`,
    transport: { type: "stdio", command: entry.transport.command, args, env: entry.transport.env.map((e) => e.name) },
    auth,
  };
}

/** A directory entry's text in the UI language. */
export const loc = (text: McpcLocalized | null | undefined) =>
  text ? (getLanguage() === "fa" ? text.fa || text.en : text.en || text.fa) : "";

/** Whether adding this entry needs a value first (a folder, a key, a token). */
export function needsForm(entry: McpcDirectoryEntry): boolean {
  if (entry.auth === "bearer" || typeof entry.auth === "object") return true;
  return entry.transport.type === "stdio" && (entry.transport.env.length > 0 || (entry.transport.argFields?.length ?? 0) > 0);
}

const isHttps = (url: string | null): url is string => !!url && /^https:\/\/[^\s]+$/i.test(url);

/** Search folds case and the Arabic/Persian letter variants a keyboard may type. */
function fold(text: string): string {
  return text.toLowerCase().replace(/ي/g, "ی").replace(/ك/g, "ک").replace(/‌/g, "");
}

// ── Dialog ────────────────────────────────────────────────────────────────────

export interface AddDialogOptions {
  /** Null while it couldn't be read; the custom form still works. */
  directory: McpcDirectoryEntry[] | null;
  directoryError: string | null;
  isAdded: (entryId: string) => boolean;
  /** After the dialog closed; focus is then the caller's to place. */
  onAdded: (id: string, name: string, secretFailed: boolean) => void;
}

let backdrop: HTMLElement | null = null;
let dialog: HTMLElement | null = null;
let restoreFocus: HTMLElement | null = null;

function close(focusBack: boolean) {
  if (!backdrop) return;
  document.removeEventListener("keydown", onKey, true);
  backdrop.remove();
  backdrop = dialog = null;
  document.body.classList.remove("wiz-open");
  if (focusBack) restoreFocus?.focus?.();
  restoreFocus = null;
}

function onKey(e: KeyboardEvent) {
  if (!dialog) return;
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    close(true);
    return;
  }
  // Keep Tab inside the dialog.
  if (e.key === "Tab") {
    const items = [...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled)")]
      .filter((el) => el.offsetParent !== null);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
}

const categoryLabel = (c: McpcCategory) => t(`mcpc.cat.${c}`);

export function openAddDialog(opts: AddDialogOptions) {
  if (backdrop) return;
  restoreFocus = document.activeElement as HTMLElement | null;

  const title = h("h2", { id: "mcpc-dlg-title", tabindex: "-1" });
  const intro = h("p", { class: "hint", id: "mcpc-dlg-intro" });
  const back = h("button", {
    type: "button", class: "mk-close mcpc-back", "aria-label": t("common.back"), title: t("common.back"),
  }, icon("chevronDown", 18)) as HTMLButtonElement;
  const closeBtn = h("button", {
    type: "button", class: "mk-close", "aria-label": t("mcpc.dlg.close"), title: t("mcpc.dlg.close"),
    onclick: () => close(true),
  }, icon("close", 18));
  const tools = h("div", { class: "mk-tools" });
  const body = h("div", { class: "mk-body" });
  const foot = h("div", { class: "mcpc-dlg-foot" });
  const live = h("span", { class: "sr-only", role: "status", "aria-live": "polite" });

  dialog = h("div", {
    class: "mk mcpc-dlg", role: "dialog", "aria-modal": "true",
    "aria-labelledby": "mcpc-dlg-title", "aria-describedby": "mcpc-dlg-intro",
  },
    h("div", { class: "mk-head" }, back, h("div", { class: "mk-titles" }, title, intro), closeBtn),
    tools,
    body,
    foot,
    live,
  );

  // ── View: the directory ──
  let query = "";
  let category: McpcCategory | "all" = "all";
  const entries = [...(opts.directory ?? [])].sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));

  function showList() {
    back.hidden = true;
    title.textContent = t("mcpc.dlg.title");
    intro.textContent = t("mcpc.dlg.intro");
    clear(tools);
    clear(body);
    clear(foot);
    foot.hidden = true;
    tools.hidden = false;
    dialog?.classList.remove("is-form");

    const search = h("input", {
      type: "search", class: "mk-search", autocomplete: "off", spellcheck: "false",
      placeholder: t("mcpc.dlg.search"), "aria-label": t("mcpc.dlg.search"), "aria-controls": "mcpc-grid",
    }) as HTMLInputElement;
    search.value = query;
    followTextDirection(search);
    const custom = h("button", { type: "button", class: "sm mcpc-custom-btn", onclick: showCustom },
      icon("plus", 14), h("span", { text: t("mcpc.dlg.custom") }));

    const present = MCPC_CATEGORIES.filter((c) => entries.some((e) => e.category === c));
    const chips = h("div", { class: "mk-cats", role: "group", "aria-label": t("mcpc.dlg.categories") });
    const chipButtons: { id: McpcCategory | "all"; el: HTMLButtonElement }[] = [];
    for (const id of ["all" as const, ...present]) {
      const el = h("button", {
        type: "button", class: "mk-cat", "aria-controls": "mcpc-grid",
        text: id === "all" ? t("mcpc.dlg.all") : categoryLabel(id),
      }) as HTMLButtonElement;
      el.addEventListener("click", () => {
        category = id;
        apply(true);
      });
      chipButtons.push({ id, el });
      chips.append(el);
    }
    tools.append(h("div", { class: "mcpc-dlg-searchrow" }, search, custom));
    if (present.length > 1) tools.append(chips);

    const grid = h("ul", { class: "mk-grid", id: "mcpc-grid" });
    const none = h("p", { class: "hint mk-none", dir: "auto" });
    const cards = entries.map((entry) => {
      const card = directoryCard(entry);
      grid.append(card);
      const words = fold(`${entry.name} ${entry.desc.fa} ${entry.desc.en} ${categoryLabel(entry.category)}`);
      return { entry, card, words };
    });
    if (opts.directoryError) {
      body.append(h("div", { class: "notice err", role: "alert", dir: "auto", text: t("mcpc.dlg.loadFailed", { err: localizeError(opts.directoryError) }) }));
    }
    body.append(grid, none);

    let liveTimer = 0;
    function apply(speak: boolean) {
      const q = fold(query.trim());
      let shown = 0;
      for (const { entry, card, words } of cards) {
        card.hidden = (category !== "all" && entry.category !== category) || (q !== "" && !words.includes(q));
        if (!card.hidden) shown++;
      }
      for (const chip of chipButtons) chip.el.setAttribute("aria-pressed", chip.id === category ? "true" : "false");
      none.hidden = shown > 0 || entries.length === 0;
      none.textContent = none.hidden ? "" : t("mcpc.dlg.noResults", { q: isolate(query.trim()) });
      if (speak) {
        window.clearTimeout(liveTimer);
        // Once typing pauses, not on every key.
        liveTimer = window.setTimeout(() => { live.textContent = t("mcpc.dlg.results", { n: shown }); }, 500);
      }
    }
    search.addEventListener("input", () => {
      query = search.value;
      apply(true);
    });
    apply(false);
    search.focus();
  }

  function directoryCard(entry: McpcDirectoryEntry): HTMLElement {
    const local = entry.transport.type === "stdio";
    // A local server's key arrives as an environment variable, but it is still a key to get.
    const secretEnv = entry.transport.type === "stdio" && entry.transport.env.some((e) => e.secret);
    const authTag = entry.auth === "oauth" ? t("mcpc.dlg.authOauth")
      : entry.auth === "none" && !secretEnv ? t("mcpc.dlg.authNone")
      : t("mcpc.dlg.authToken");
    const tags = h("div", { class: "mcpc-tags" },
      h("span", { class: "chip", text: t(local ? "mcpc.dlg.local" : "mcpc.dlg.remote") }),
      h("span", { class: "chip", text: authTag }),
      entry.needs ? h("span", { class: "chip", text: t("mcpc.dlg.needs", { what: isolate(cleanText(entry.needs, 40)) }) }) : null,
    );
    const footEl = h("div", { class: "mk-card-foot mcpc-card-foot" });
    const docs = entry.docsUrl;
    if (isHttps(docs)) {
      footEl.append(h("button", { type: "button", class: "link", text: t("mcpc.dlg.docs"), onclick: () => void Bridge.openUrl(docs) }));
    }
    footEl.append(h("span", { class: "spacer" }));
    const added = opts.isAdded(entry.id);
    // A second folder (or another argument) is a second server, so those stay addable.
    const repeatable = entry.transport.type === "stdio" && (entry.transport.argFields?.length ?? 0) > 0;
    if (added) footEl.append(statusBadge("ok", t("mcpc.dlg.added")));
    if (!added || repeatable) {
      const again = added && repeatable;
      const add = h("button", { type: "button", class: "sm", "aria-label": t(again ? "mcpc.dlg.addAnotherNamed" : "mcpc.dlg.addNamed", { name: entry.name }) },
        icon("plus", 14), h("span", { text: t(again ? "mcpc.dlg.addAnother" : "mcpc.dlg.add") })) as HTMLButtonElement;
      add.addEventListener("click", () => {
        if (needsForm(entry)) showConfigure(entry);
        else void submit(add, directorySpec(entry, {}), [], null);
      });
      footEl.append(add);
    }
    return h("li", { class: "mk-card" },
      h("div", { class: "mk-card-top" },
        h("span", { class: "mk-name", dir: "auto", text: entry.name }),
        h("span", { class: "mk-cat-label", text: categoryLabel(entry.category) }),
      ),
      h("p", { class: "mk-desc", dir: "auto", text: loc(entry.desc) }),
      tags,
      footEl,
      h("p", { class: "int-test-err mcpc-card-err", role: "alert", dir: "auto" }),
    );
  }

  /** Switches the dialog to a form; `submitLabel` reads the same for both kinds. */
  function formView(heading: string, sub: string): { form: HTMLFormElement; actions: HTMLElement; error: HTMLElement } {
    back.hidden = false;
    title.textContent = heading;
    intro.textContent = sub;
    clear(tools);
    tools.hidden = true;
    clear(body);
    clear(foot);
    foot.hidden = false;
    dialog?.classList.add("is-form");
    const form = h("form", { class: "mcpc-form", novalidate: true, id: "mcpc-dlg-form" }) as HTMLFormElement;
    const error = h("div", { class: "notice err", role: "alert", dir: "auto" });
    error.hidden = true;
    body.append(form);
    const actions = h("div", { class: "actions" });
    foot.append(error, actions);
    queueMicrotask(() => (form.querySelector<HTMLElement>("input, select, textarea") ?? title).focus({ preventScroll: true }));
    return { form, actions, error };
  }

  /** A labelled control with its own error line. */
  function fieldBox(id: string, label: string, control: HTMLElement, hint?: string | null): { el: HTMLElement; setError: (msg?: string) => void } {
    control.id = id;
    const err = h("div", { class: "field-err", id: `${id}-err`, dir: "auto" });
    const hintEl = hint ? h("p", { class: "key-help-text", id: `${id}-hint`, text: hint }) : null;
    control.setAttribute("aria-describedby", [hint ? `${id}-hint` : "", `${id}-err`].filter(Boolean).join(" "));
    // Typing is the fix; the message goes with it.
    control.addEventListener("input", () => {
      err.textContent = "";
      err.removeAttribute("role");
      control.removeAttribute("aria-invalid");
    });
    return {
      el: h("div", { class: "field" }, h("label", { for: id, text: label }), control, hintEl, err),
      setError: (msg?: string) => {
        err.textContent = msg ?? "";
        if (msg) err.setAttribute("role", "alert");
        else err.removeAttribute("role");
        if (msg) control.setAttribute("aria-invalid", "true");
        else control.removeAttribute("aria-invalid");
      },
    };
  }

  const ltrInput = (attrs: Record<string, string> = {}) =>
    h("input", { type: "text", dir: "ltr", autocomplete: "off", spellcheck: "false", ...attrs }) as HTMLInputElement;
  const secretInput = (placeholder: string) =>
    h("input", { type: "password", dir: "ltr", autocomplete: "off", spellcheck: "false", placeholder: placeholder || t("mcpc.paste") }) as HTMLInputElement;

  // ── View: a directory entry that needs a value first ──
  function showConfigure(entry: McpcDirectoryEntry) {
    const { form, actions, error } = formView(t("mcpc.dlg.configTitle", { name: entry.name }), loc(entry.desc));
    const argInputs: { index: number; input: HTMLInputElement; setError: (m?: string) => void }[] = [];
    const secrets: { slot: McpcSecretSlot; input: HTMLInputElement }[] = [];
    const required: { input: HTMLInputElement; setError: (m?: string) => void }[] = [];
    let preview: HTMLElement | null = null;

    if (entry.needs) form.append(h("p", { class: "hint", text: t("mcpc.dlg.needs", { what: isolate(cleanText(entry.needs, 40)) }) }));

    if (entry.transport.type === "stdio") {
      for (const f of entry.transport.argFields ?? []) {
        const input = ltrInput({ placeholder: f.placeholder });
        const box = fieldBox(`mcpc-arg-${f.index}`, loc(f.label), input);
        argInputs.push({ index: f.index, input, setError: box.setError });
        form.append(box.el);
      }
      for (const env of entry.transport.env) {
        const input = env.secret ? secretInput(env.placeholder) : ltrInput({ placeholder: env.placeholder || t("mcpc.paste") });
        // Required: the server can't start without it. Short label; the variable is shown under the field.
        const box = fieldBox(`mcpc-env-${env.name}`, loc(env.label) || env.name, input, t("mcpc.dlg.envVar", { name: isolate(env.name) }));
        input.required = true;
        form.append(box.el);
        secrets.push({ slot: `env:${env.name}`, input });
        required.push({ input, setError: box.setError });
      }
    }
    if (entry.auth === "bearer" || typeof entry.auth === "object") {
      const input = secretInput("");
      const label = typeof entry.auth === "object" ? t("mcpc.headerValue", { header: isolate(entry.auth.header) }) : t("mcpc.token");
      form.append(fieldBox("mcpc-token", t("mcpc.dlg.optional", { field: label }), input, loc(entry.authHelp) || null).el);
      secrets.push({ slot: "token", input });
    }
    if (secrets.length > required.length) form.append(h("p", { class: "storage-note", text: t("mcpc.dlg.keysLater") }));
    if (isHttps(entry.docsUrl)) {
      const docs = entry.docsUrl;
      form.append(h("div", { class: "key-help" }, h("button", { type: "button", class: "link", text: t("mcpc.dlg.docs"), onclick: () => void Bridge.openUrl(docs) })));
    }

    if (entry.transport.type === "stdio") {
      // Exactly what will be approved later, kept in step with the folder typed.
      preview = h("pre", { class: "mcpc-code", dir: "ltr" });
      const sync = () => {
        const values: Record<number, string> = {};
        for (const a of argInputs) values[a.index] = a.input.value;
        const spec = directorySpec(entry, values);
        if (spec.transport.type === "stdio" && preview) preview.textContent = commandLine(spec.transport.command, spec.transport.args);
      };
      for (const a of argInputs) a.input.addEventListener("input", sync);
      sync();
      form.append(h("div", { class: "mcpc-cmd" },
        h("span", { class: "field-label", text: t("mcpc.cmdTitle") }),
        preview,
        h("p", { class: "key-help-text", text: t("mcpc.dlg.stdioNote") }),
      ));
    }

    const submitBtn = h("button", { type: "submit", class: "primary", form: "mcpc-dlg-form", text: t("mcpc.dlg.submit") }) as HTMLButtonElement;
    actions.append(submitBtn, h("button", { type: "button", text: t("common.cancel"), onclick: showList }));
    back.onclick = showList;

    form.addEventListener("submit", (e) => {
      e.preventDefault();
      error.hidden = true;
      const values: Record<number, string> = {};
      let firstBad: HTMLInputElement | null = null;
      for (const a of argInputs) {
        const v = a.input.value.trim();
        const msg = !v ? t("mcpc.err.required") : LINE_BREAK.test(v) ? t("mcpc.err.lineBreak") : undefined;
        a.setError(msg);
        if (msg && !firstBad) firstBad = a.input;
        values[a.index] = v;
      }
      for (const r of required) {
        const msg = r.input.value.trim() ? undefined : t("mcpc.err.required");
        r.setError(msg);
        if (msg && !firstBad) firstBad = r.input;
      }
      if (firstBad) {
        firstBad.focus();
        return;
      }
      // Added off, and switched on only once its required values are stored.
      const spec = { ...directorySpec(entry, values), enabled: required.length === 0 };
      const requiredSlots = secrets.filter((x) => x.input.required).map((x) => x.slot);
      void submit(submitBtn, spec, secrets.map((x) => ({ slot: x.slot, value: x.input.value.trim() })), error, requiredSlots);
    });
  }

  // ── View: a custom server ──
  function showCustom() {
    const { form, actions, error } = formView(t("mcpc.dlg.custom"), t("mcpc.dlg.customHint"));
    let kind: CustomForm["kind"] = "http";

    const name = h("input", { type: "text", autocomplete: "off", maxlength: "60" }) as HTMLInputElement;
    followTextDirection(name);
    const nameBox = fieldBox("mcpc-f-name", t("mcpc.form.name"), name);

    const kindGroup = h("div", { class: "segmented mcpc-seg2", role: "radiogroup", "aria-labelledby": "mcpc-f-kind" });
    const kindButtons = new Map<CustomForm["kind"], HTMLButtonElement>();
    for (const k of ["http", "stdio"] as const) {
      const b = h("button", { type: "button", class: "seg", role: "radio", text: t(k === "http" ? "mcpc.form.typeHttp" : "mcpc.form.typeStdio") }) as HTMLButtonElement;
      b.addEventListener("click", () => setKind(k));
      b.addEventListener("keydown", (e) => {
        if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) return;
        e.preventDefault();
        const next = k === "http" ? "stdio" : "http";
        setKind(next);
        kindButtons.get(next)?.focus();
      });
      kindButtons.set(k, b);
      kindGroup.append(b);
    }

    const url = ltrInput({ placeholder: "https://example.com/mcp", inputmode: "url" });
    const urlBox = fieldBox("mcpc-f-url", t("mcpc.form.url"), url);

    const authSel = h("select", {}) as HTMLSelectElement;
    for (const [value, key] of [["none", "mcpc.form.authNone"], ["bearer", "mcpc.form.authBearer"], ["header", "mcpc.form.authHeader"], ["oauth", "mcpc.form.authOauth"]] as const) {
      authSel.append(h("option", { value, text: t(key) }));
    }
    const authBox = fieldBox("mcpc-f-auth", t("mcpc.form.auth"), authSel);
    const header = ltrInput({ placeholder: "X-API-Key", maxlength: "64" });
    const headerBox = fieldBox("mcpc-f-header", t("mcpc.form.headerName"), header);
    const token = secretInput("");
    const tokenLabel = h("span");
    const tokenBox = fieldBox("mcpc-f-token", "", token);
    tokenBox.el.querySelector("label")?.append(tokenLabel);

    const command = ltrInput({ placeholder: "npx" });
    const commandBox = fieldBox("mcpc-f-command", t("mcpc.form.command"), command);
    const args = h("textarea", { dir: "ltr", rows: "4", spellcheck: "false", placeholder: "-y\n@scope/server-name" }) as HTMLTextAreaElement;
    const argsBox = fieldBox("mcpc-f-args", t("mcpc.form.args"), args);
    const env = h("textarea", { dir: "ltr", rows: "2", spellcheck: "false", placeholder: "API_KEY" }) as HTMLTextAreaElement;
    const envBox = fieldBox("mcpc-f-env", t("mcpc.form.env"), env, t("mcpc.form.envHint"));
    const preview = h("pre", { class: "mcpc-code", dir: "ltr" });
    const previewBox = h("div", { class: "mcpc-cmd" },
      h("span", { class: "field-label", text: t("mcpc.cmdTitle") }), preview,
      h("p", { class: "key-help-text", text: t("mcpc.dlg.stdioNote") }));

    form.append(
      nameBox.el,
      h("div", { class: "field" }, h("span", { class: "field-label", id: "mcpc-f-kind", text: t("mcpc.form.type") }), kindGroup),
      urlBox.el, authBox.el, headerBox.el, tokenBox.el,
      commandBox.el, argsBox.el, envBox.el, previewBox,
    );

    function setKind(next: CustomForm["kind"]) {
      kind = next;
      for (const [k, b] of kindButtons) {
        const on = k === kind;
        b.classList.toggle("on", on);
        b.setAttribute("aria-checked", on ? "true" : "false");
        b.tabIndex = on ? 0 : -1;
      }
      syncFields();
    }
    function syncFields() {
      const http = kind === "http";
      const auth = authSel.value as McpcAuthType;
      urlBox.el.hidden = authBox.el.hidden = !http;
      headerBox.el.hidden = !http || auth !== "header";
      tokenBox.el.hidden = !http || (auth !== "bearer" && auth !== "header");
      tokenLabel.textContent = t("mcpc.dlg.optional", {
        field: auth === "header" && header.value.trim() ? t("mcpc.headerValue", { header: isolate(header.value.trim()) }) : t("mcpc.token"),
      });
      commandBox.el.hidden = argsBox.el.hidden = envBox.el.hidden = previewBox.hidden = http;
      preview.textContent = commandLine(command.value.trim() || "…", lines(args.value));
    }
    authSel.addEventListener("change", syncFields);
    header.addEventListener("input", syncFields);
    command.addEventListener("input", syncFields);
    args.addEventListener("input", syncFields);
    setKind("http");

    const submitBtn = h("button", { type: "submit", class: "primary", form: "mcpc-dlg-form", text: t("mcpc.dlg.submit") }) as HTMLButtonElement;
    actions.append(submitBtn, h("button", { type: "button", text: t("common.cancel"), onclick: showList }));
    back.onclick = showList;

    const boxes: Record<CustomField, { setError: (m?: string) => void; control: HTMLElement }> = {
      name: { ...nameBox, control: name }, url: { ...urlBox, control: url }, command: { ...commandBox, control: command },
      args: { ...argsBox, control: args }, env: { ...envBox, control: env }, header: { ...headerBox, control: header },
    };
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      error.hidden = true;
      const { spec, errors } = customSpec({
        name: name.value, kind, url: url.value, command: command.value, args: args.value, env: env.value,
        auth: authSel.value as McpcAuthType, header: header.value,
      });
      let first: HTMLElement | null = null;
      for (const [field, box] of Object.entries(boxes) as [CustomField, (typeof boxes)[CustomField]][]) {
        box.setError(errors[field]);
        if (errors[field] && !first) first = box.control;
      }
      if (!spec) {
        first?.focus();
        return;
      }
      const tokenValue = !tokenBox.el.hidden ? token.value.trim() : "";
      void submit(submitBtn, spec, tokenValue ? [{ slot: "token", value: tokenValue }] : [], error);
    });
  }

  /** Adds the server, then stores the values typed for it; a key that fails doesn't undo the add. */
  async function submit(
    button: HTMLButtonElement, spec: McpcAddSpec,
    secrets: { slot: McpcSecretSlot; value: string }[], error: HTMLElement | null,
    requiredSlots: McpcSecretSlot[] = [],
  ) {
    const label = button.textContent ?? "";
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    if (error) button.textContent = t("mcpc.dlg.adding");
    let id: string;
    try {
      id = await BridgeMcpc.add(spec);
    } catch (err) {
      button.disabled = false;
      button.removeAttribute("aria-busy");
      button.textContent = label;
      const msg = t("mcpc.dlg.addFailed", { err: localizeError(err) });
      if (error) {
        error.textContent = msg;
        error.hidden = false;
      } else {
        // One slot per card, so a retry replaces the message rather than stacking it.
        const slot = button.closest(".mk-card")?.querySelector<HTMLElement>(".mcpc-card-err");
        if (slot) slot.textContent = msg;
        else live.textContent = msg;
      }
      return;
    }
    let secretFailed = false;
    const stored = new Set<McpcSecretSlot>();
    for (const s of secrets) {
      if (!s.value) continue;
      try {
        await BridgeMcpc.setSecret(id, s.slot, s.value);
        stored.add(s.slot);
      } catch {
        secretFailed = true;
      }
    }
    // A server with required values was added off; it goes on once those are all stored.
    if (spec.enabled === false && requiredSlots.length > 0 && requiredSlots.every((slot) => stored.has(slot))) {
      try {
        await BridgeMcpc.update(id, { enabled: true });
      } catch (err) {
        void Bridge.log(`settings: mcp server ${id} not switched on after add: ${String(err).split(/[|\n]/, 1)[0]}`);
      }
    }
    void Bridge.log(`settings: mcp server added ${id} (${spec.source}, ${spec.transport.type})`);
    close(false);
    opts.onAdded(id, spec.name, secretFailed);
  }

  backdrop = h("div", { class: "mk-backdrop" }, dialog);
  backdrop.addEventListener("mousedown", (e) => {
    if (e.target === backdrop) close(true);
  });
  document.body.append(backdrop);
  // Shares the wizard's scroll lock.
  document.body.classList.add("wiz-open");
  document.addEventListener("keydown", onKey, true);
  if (entries.length === 0 && !opts.directoryError) showCustom();
  else showList();
  void Bridge.log(`settings: mcp server directory opened (${entries.length} entries)`);
}
