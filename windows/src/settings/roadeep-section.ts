// Settings window — the Roadeep account (sign-in, model, features). It also
// reads what the account offers (models, plan locks, Roadeep's agents) for the
// "Agents" section (agents-section.ts), which listens through
// `onRoadeepData`.
//
// The section redraws from `rd` and from the small UI records below, so a
// redraw (language change, sign-in from the other window, a list refresh)
// never loses what the user was typing. Passwords are the exception on
// purpose: they live only in their <input> and are cleared after each submit.

import {
  Bridge, isRoadeepError, onRoadeepSession,
  type RoadeepAccountStatus, type RoadeepAgentCatalog, type RoadeepModel, type RoadeepSession,
} from "../core/bridge";
import { REASONING_EFFORTS, type Settings } from "../core/state";
import { formatNumber, isolate, isRtl, t } from "../core/i18n";
import { h, clear } from "../views/dom";
import { fieldErrors, roadeepErrorText } from "../views/errors";
import { field, icon, linkButton, sectionHead, settingRow, statusBadge, switchEl, helpDisclosure } from "./ui";

const ROADEEP_SITE = "https://roadeep.com";
const OTP_COOLDOWN_S = 60;

export interface RoadeepSectionHost {
  settings: () => Settings;
  save: () => Promise<void>;
}

let host: RoadeepSectionHost;

/** Everything the account section, and the agents section, are drawn from. */
const rd = {
  /** null while the first read is in flight. */
  session: null as RoadeepSession | null,
  expired: false,
  models: null as RoadeepModel[] | null,
  modelsError: null as string | null,
  status: null as RoadeepAccountStatus | null,
  statusError: null as string | null,
  catalog: null as RoadeepAgentCatalog | null,
  catalogError: null as string | null,
};

export type RoadeepData = Readonly<typeof rd>;

const dataListeners = new Set<() => void>();

/** Read-only view of the account data, for the other sections. */
export function roadeepData(): RoadeepData {
  return rd;
}

/** Called whenever the account data changes (sign-in, a list read, sign-out). */
export function onRoadeepData(fn: () => void): void {
  dataListeners.add(fn);
}

function dataChanged() {
  for (const fn of dataListeners) fn();
}

export function openRoadeepSite() {
  void Bridge.openUrl(ROADEEP_SITE);
}

const login = {
  tab: "email" as "email" | "phone",
  email: "",
  phone: "",
  code: "",
  codeSent: false,
  cooldownUntil: 0,
  pending: false,
};

const accountSlot = h("div", { class: "slot" });

function log(message: string) {
  void Bridge.log(`settings: ${message}`);
}

function errCode(err: unknown): string {
  return isRoadeepError(err) ? `${err.code}${err.requestId ? ` ${err.requestId}` : ""}` : String(err);
}

const openSite = openRoadeepSite;

// ── Session ───────────────────────────────────────────────────────────────────

function applySession(session: RoadeepSession) {
  const wasSignedIn = rd.session?.signedIn === true;
  rd.session = session;
  rd.expired = !session.signedIn && session.reason === "expired";
  if (!session.signedIn) {
    rd.models = rd.status = rd.catalog = null;
    rd.modelsError = rd.statusError = rd.catalogError = null;
  }
  if (session.signedIn && !wasSignedIn) {
    login.code = "";
    login.codeSent = false;
    void loadAccountData();
  }
  drawAccount();
  dataChanged();
}

async function loadAccountData() {
  const [models, status, agents] = await Promise.allSettled([
    Bridge.roadeepModels(),
    Bridge.roadeepProfileLocks(),
    Bridge.roadeepAgentCatalog(),
  ]);
  if (!rd.session?.signedIn) return;
  if (models.status === "fulfilled") {
    rd.models = models.value;
    rd.modelsError = null;
  } else {
    rd.modelsError = roadeepErrorText(models.reason);
    log(`models failed ${errCode(models.reason)}`);
  }
  if (status.status === "fulfilled") {
    rd.status = status.value;
    rd.statusError = null;
  } else {
    rd.statusError = roadeepErrorText(status.reason);
    log(`profile failed ${errCode(status.reason)}`);
  }
  if (agents.status === "fulfilled") {
    rd.catalog = agents.value;
    rd.catalogError = null;
  } else {
    rd.catalogError = roadeepErrorText(agents.reason);
    log(`agents failed ${errCode(agents.reason)}`);
  }
  drawAccount();
  dataChanged();
}

/** Builds the slot once and starts listening; call before the first render. */
export function initRoadeepSections(h0: RoadeepSectionHost) {
  host = h0;
  void onRoadeepSession(applySession);
  Bridge.roadeepSession().then(applySession, (err) => {
    log(`session read failed ${errCode(err)}`);
    applySession({ signedIn: false, user: null, reason: null });
  });
  window.setInterval(tickCooldown, 1000);
}

/** The account section; stays the same element across redraws. */
export function accountSection(): HTMLElement {
  drawAccount();
  return accountSlot;
}

// ── Account: signed out ───────────────────────────────────────────────────────

function drawAccount() {
  clear(accountSlot);
  const signedIn = rd.session?.signedIn === true;
  const badge = rd.session == null
    ? null
    : statusBadge(signedIn ? "ok" : rd.expired ? "warn" : "off", t(signedIn ? "status.connected" : "status.signedOut"));
  const section = h(
    "section",
    { class: "sec account", "aria-labelledby": "sec-account-title" },
    sectionHead({ id: "sec-account-title", icon: "account", title: t("account.title"), desc: t("section.accountDesc"), badge }),
  );
  if (rd.session == null) {
    section.append(h("div", { class: "card" }, h("p", { class: "hint", text: t("account.checking") })));
  } else if (signedIn) {
    drawSignedIn(section);
  } else {
    drawSignedOut(section);
  }
  accountSlot.append(section);
}

function drawSignedOut(section: HTMLElement) {
  const card = h("div", { class: "card login-card" },
    helpDisclosure(t("account.intro"), `${t("settings.help")}: ${t("account.title")}`),
  );
  if (rd.expired) card.append(h("div", { class: "notice warn", role: "status", text: t("account.expired") }));

  // ARIA tabs: roving tabindex, arrow keys / Home / End switch the method.
  const TABS = ["email", "phone"] as const;
  const tabs = h("div", { class: "tabs", role: "tablist", "aria-label": t("account.title") });
  const pane = h("div", {
    class: "tab-pane", role: "tabpanel", id: "rd-tabpanel", "aria-labelledby": `rd-tab-${login.tab}`,
  });
  const select = (id: (typeof TABS)[number]) => {
    if (login.pending || login.tab === id) return;
    login.tab = id;
    drawAccount();
  };
  for (const id of TABS) {
    const on = login.tab === id;
    const tab = h("button", {
      type: "button",
      id: `rd-tab-${id}`,
      class: on ? "tab on" : "tab",
      role: "tab",
      "aria-selected": on ? "true" : "false",
      "aria-controls": "rd-tabpanel",
      tabindex: on ? "0" : "-1",
      text: t(id === "email" ? "account.tabEmail" : "account.tabPhone"),
    });
    tab.addEventListener("click", () => select(id));
    tab.addEventListener("keydown", (e) => {
      const i = TABS.indexOf(id);
      let next: number | null = null;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") next = (i + 1) % TABS.length;
      else if (e.key === "Home") next = 0;
      else if (e.key === "End") next = TABS.length - 1;
      if (next == null) return;
      e.preventDefault();
      select(TABS[next]);
      // After the form's own autofocus, which drawAccount queued first.
      queueMicrotask(() => document.getElementById(`rd-tab-${login.tab}`)?.focus({ preventScroll: true }));
    });
    tabs.append(tab);
  }
  pane.append(login.tab === "email" ? emailForm() : phoneForm());

  card.append(
    tabs,
    pane,
    h("div", { class: "links" },
      linkButton(t("account.createAccount"), openSite),
      h("span", { class: "sep", "aria-hidden": "true", text: "·" }),
      linkButton(t("account.forgotPassword"), openSite),
    ),
  );
  section.append(card);
}

/** Disables every control of a form while a request is out. */
function setPending(form: HTMLFormElement, pending: boolean, submit: HTMLButtonElement, label: string) {
  login.pending = pending;
  for (const el of Array.from(form.elements) as (HTMLInputElement | HTMLButtonElement)[]) el.disabled = pending;
  submit.textContent = label;
  form.setAttribute("aria-busy", pending ? "true" : "false");
}

function showFormError(slot: HTMLElement, err: unknown) {
  clear(slot);
  slot.append(h("div", { class: "notice err", role: "alert", dir: "auto", text: roadeepErrorText(err) }));
}

function emailForm(): HTMLElement {
  const email = h("input", {
    type: "email", dir: "ltr", autocomplete: "username", spellcheck: "false",
    value: login.email, placeholder: "name@example.com",
  }) as HTMLInputElement;
  const password = h("input", { type: "password", dir: "ltr", autocomplete: "current-password" }) as HTMLInputElement;
  const submit = h("button", { type: "submit", class: "primary block", text: t("account.signIn") }) as HTMLButtonElement;
  const general = h("div", {});
  const emailField = field("rd-email", t("account.email"), email);
  const passwordField = field("rd-password", t("account.password"), password);
  const reveal = h("button", {
    type: "button", class: "reveal", "aria-label": t("account.showPassword"), title: t("account.showPassword"),
    "aria-pressed": "false", "aria-controls": "rd-password",
  }, icon("eye", 16)) as HTMLButtonElement;
  reveal.addEventListener("click", () => {
    const show = password.type === "password";
    password.type = show ? "text" : "password";
    reveal.setAttribute("aria-pressed", show ? "true" : "false");
    clear(reveal);
    reveal.append(icon(show ? "eyeOff" : "eye", 16));
  });
  const passwordWrap = h("div", { class: "input-wrap" });
  password.replaceWith(passwordWrap);
  passwordWrap.append(password, reveal);
  const form = h("form", { class: "login-form", novalidate: true },
    emailField, passwordField, h("div", { class: "form-actions" }, submit), general) as HTMLFormElement;

  email.addEventListener("input", () => { login.email = email.value; });

  const setFieldError = (wrap: HTMLElement, input: HTMLInputElement, text?: string) => {
    wrap.querySelector(".field-err")!.textContent = text ?? "";
    if (text) input.setAttribute("aria-invalid", "true");
    else input.removeAttribute("aria-invalid");
  };

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (login.pending) return;
    clear(general);
    const address = email.value.trim();
    const secret = password.value;
    setFieldError(emailField, email, address.includes("@") ? undefined : t("account.emailRequired"));
    setFieldError(passwordField, password, secret ? undefined : t("account.passwordRequired"));
    if (!address.includes("@")) return email.focus({ preventScroll: true });
    if (!secret) return password.focus({ preventScroll: true });

    // The password leaves the field the moment it is sent.
    password.value = "";
    setPending(form, true, submit, t("account.signingIn"));
    try {
      applySession(await Bridge.roadeepLogin(address, secret));
    } catch (err) {
      log(`login failed ${errCode(err)}`);
      setPending(form, false, submit, t("account.signIn"));
      const fields = fieldErrors(err);
      setFieldError(emailField, email, fields.email);
      setFieldError(passwordField, password, fields.password);
      showFormError(general, err);
      password.focus({ preventScroll: true });
    }
  });
  queueMicrotask(() => (login.email ? password : email).focus({ preventScroll: true }));
  return form;
}

function cooldownLeft(): number {
  return Math.max(0, Math.ceil((login.cooldownUntil - Date.now()) / 1000));
}

/** Keeps the resend button's countdown live without redrawing the form. */
function tickCooldown() {
  const btn = accountSlot.querySelector<HTMLButtonElement>("button[data-resend]");
  if (!btn) return;
  const left = cooldownLeft();
  btn.textContent = left > 0 ? t("account.resendIn", { s: left }) : t("account.resend");
  btn.disabled = left > 0 || login.pending;
}

function startCooldown(err?: unknown) {
  const wait = isRoadeepError(err) && err.code === "THROTTLED" ? (err.retryAfter ?? OTP_COOLDOWN_S) : OTP_COOLDOWN_S;
  login.cooldownUntil = Date.now() + wait * 1000;
}

function phoneForm(): HTMLElement {
  const general = h("div", {});
  if (!login.codeSent) {
    const phone = h("input", {
      type: "tel", dir: "ltr", inputmode: "tel", autocomplete: "tel", spellcheck: "false",
      value: login.phone, placeholder: "09xxxxxxxxx",
    }) as HTMLInputElement;
    const submit = h("button", { type: "submit", class: "primary block", text: t("account.sendCode") }) as HTMLButtonElement;
    const phoneField = field("rd-phone", t("account.phone"), phone);
    const form = h("form", { class: "login-form", novalidate: true },
      phoneField, h("div", { class: "form-actions" }, submit), general) as HTMLFormElement;
    phone.addEventListener("input", () => { login.phone = phone.value; });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (login.pending) return;
      clear(general);
      const number = phone.value.trim();
      const errBox = phoneField.querySelector(".field-err")!;
      errBox.textContent = number ? "" : t("account.phoneRequired");
      if (!number) return phone.focus({ preventScroll: true });
      if (cooldownLeft() > 0) {
        errBox.textContent = t("rerr.THROTTLED.wait", { s: cooldownLeft() });
        return;
      }
      setPending(form, true, submit, t("account.sending"));
      try {
        await Bridge.roadeepOtpSend(number);
        login.phone = number;
        login.codeSent = true;
        login.code = "";
        startCooldown();
        login.pending = false;
        drawAccount();
      } catch (err) {
        log(`otp send failed ${errCode(err)}`);
        if (isRoadeepError(err) && err.code === "THROTTLED") startCooldown(err);
        setPending(form, false, submit, t("account.sendCode"));
        errBox.textContent = fieldErrors(err).phone ?? "";
        showFormError(general, err);
        phone.focus({ preventScroll: true });
      }
    });
    queueMicrotask(() => phone.focus({ preventScroll: true }));
    return form;
  }

  const code = h("input", {
    type: "text", dir: "ltr", inputmode: "numeric", autocomplete: "one-time-code",
    maxlength: "8", spellcheck: "false", value: login.code, class: "otp",
  }) as HTMLInputElement;
  const submit = h("button", { type: "submit", class: "primary block", text: t("account.verify") }) as HTMLButtonElement;
  const resend = h("button", { type: "button", class: "block", "data-resend": "1" }) as HTMLButtonElement;
  const codeField = field("rd-otp", t("account.code"), code);
  const form = h("form", { class: "login-form", novalidate: true },
    h("div", { class: "hint", role: "status" },
      t("account.codeSent", { phone: isolate(login.phone) }), " ",
      linkButton(t("account.changePhone"), () => {
        if (login.pending) return;
        login.codeSent = false;
        login.code = "";
        drawAccount();
      }),
    ),
    codeField,
    h("div", { class: "form-actions" }, submit, resend),
    general,
  ) as HTMLFormElement;
  code.addEventListener("input", () => { login.code = code.value; });

  resend.addEventListener("click", async () => {
    if (login.pending || cooldownLeft() > 0) return;
    clear(general);
    login.pending = true;
    tickCooldown();
    try {
      await Bridge.roadeepOtpSend(login.phone);
      startCooldown();
    } catch (err) {
      log(`otp resend failed ${errCode(err)}`);
      if (isRoadeepError(err) && err.code === "THROTTLED") startCooldown(err);
      showFormError(general, err);
    } finally {
      login.pending = false;
      tickCooldown();
    }
  });

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (login.pending) return;
    clear(general);
    const otp = code.value.trim();
    const errBox = codeField.querySelector(".field-err")!;
    errBox.textContent = otp ? "" : t("account.codeRequired");
    if (!otp) return code.focus({ preventScroll: true });
    setPending(form, true, submit, t("account.verifying"));
    try {
      applySession(await Bridge.roadeepOtpVerify(login.phone, otp));
    } catch (err) {
      log(`otp verify failed ${errCode(err)}`);
      setPending(form, false, submit, t("account.verify"));
      tickCooldown();
      errBox.textContent = fieldErrors(err).otp ?? "";
      showFormError(general, err);
      code.select();
    }
  });
  queueMicrotask(() => {
    tickCooldown();
    code.focus({ preventScroll: true });
  });
  return form;
}

// ── Account: signed in ────────────────────────────────────────────────────────

/** Only a Roadeep-hosted picture is loaded; anything else gets the initial. */
function avatar(name: string, url: string | null): HTMLElement {
  const box = h("div", { class: "avatar", "aria-hidden": "true" });
  let trusted = false;
  try {
    const u = url ? new URL(url) : null;
    trusted = !!u && u.protocol === "https:" && (u.hostname === "roadeep.com" || u.hostname.endsWith(".roadeep.com"));
  } catch {
    trusted = false;
  }
  if (trusted && url) {
    const img = h("img", { src: url, alt: "" }) as HTMLImageElement;
    img.addEventListener("error", () => {
      img.remove();
      box.textContent = initial(name);
    });
    box.append(img);
  } else {
    box.textContent = initial(name);
  }
  return box;
}

function initial(name: string): string {
  return (Array.from(name.trim())[0] ?? "?").toLocaleUpperCase();
}

function drawSignedIn(section: HTMLElement) {
  const user = rd.session?.user;
  const name = user?.name?.trim() || user?.email || user?.phone || "Roadeep";
  const contact = [user?.email, user?.phone].filter((v): v is string => !!v && v !== name);
  const feedback = h("div", {});

  const signOut = h("button", { type: "button", class: "sm" },
    icon("logout", 16), h("span", { text: t("account.signOut") })) as HTMLButtonElement;
  signOut.addEventListener("click", async () => {
    signOut.disabled = true;
    try {
      await Bridge.roadeepLogout();
      applySession({ signedIn: false, user: null, reason: "logout" });
    } catch (err) {
      log(`logout failed ${errCode(err)}`);
      signOut.disabled = false;
      showFormError(feedback, err);
    }
  });

  const status = rd.status;
  const chips = h("div", { class: "chips" });
  if (status?.planName) {
    chips.append(h("span", { class: "chip" },
      h("b", { text: `${t("account.plan")}:` }), " ", h("bdi", { text: status.planName })));
  }
  if (status?.walletUnits != null) {
    chips.append(h("span", { class: "chip", text: t("account.balance", { n: formatNumber(status.walletUnits, { useGrouping: true }) }) }));
  }
  const who = h("div", { class: "who" },
    h("div", { class: "name", dir: "auto", text: name }),
    // Each item isolated, so email and phone keep the reading order in fa and en.
    contact.length
      ? h("div", { class: "contact" }, ...contact.flatMap((v, i) => [i ? " · " : "", h("bdi", { dir: "ltr", text: v })]))
      : null,
    chips.childElementCount ? chips : null,
  );
  const profile = h("div", { class: "card account-card" }, avatar(name, user?.avatar ?? null), who, signOut);
  section.append(profile, feedback);
  if (rd.statusError) section.append(h("div", { class: "notice warn", text: t("account.statusFailed", { err: rd.statusError }) }));

  section.append(modelRow(), featuresGroup());
}

function modelLabel(m: RoadeepModel): string {
  return m.displayName || m.id;
}

function modelRow(): HTMLElement {
  const s = host.settings();
  const select = h("select", { id: "rd-model" }) as HTMLSelectElement;
  const fallback = rd.models?.find((m) => m.isDefault);
  select.append(h("option", {
    value: "",
    text: fallback ? t("account.serverDefaultNamed", { name: modelLabel(fallback) }) : t("account.serverDefault"),
  }));
  for (const m of rd.models ?? []) select.append(h("option", { value: m.id, text: modelLabel(m) }));
  if (s.model && !(rd.models ?? []).some((m) => m.id === s.model)) {
    select.append(h("option", {
      value: s.model,
      text: rd.models ? t("account.modelUnavailable", { id: s.model }) : s.model,
    }));
  }
  select.value = s.model;
  select.disabled = rd.models == null && !rd.modelsError;
  select.addEventListener("change", () => {
    host.settings().model = select.value;
    void host.save();
    drawAccount();
  });
  const wrap = h("div", { class: "card list" },
    settingRow({ label: t("account.model"), hint: t("account.modelHint"), forId: "rd-model" }, select));
  if (rd.modelsError) wrap.append(h("div", { class: "notice err", text: t("account.modelsFailed", { err: rd.modelsError }) }));
  return wrap;
}

type Feature = "webSearch" | "reasoning" | "deepResearch";

/** The model the next new thread will use, when the catalogue knows it. */
function currentModel(): RoadeepModel | undefined {
  const id = host.settings().model;
  return rd.models?.find((m) => (id ? m.id === id : m.isDefault));
}

function gate(feature: Feature): { available: boolean; locked: boolean } {
  const m = currentModel();
  const locks = rd.status?.locks;
  switch (feature) {
    case "webSearch":
      return { available: m ? m.webSearch : true, locked: locks?.webSearch === true };
    case "deepResearch":
      return { available: m ? m.deepResearch : true, locked: locks?.webSearch === true };
    case "reasoning":
      return { available: m ? m.reasoning : true, locked: locks?.reasoning === true };
  }
}

const usable = (g: { available: boolean; locked: boolean }) => g.available && !g.locked;

/**
 * Same rules as chat.rs: web search and reasoning exclude each other, deep
 * research turns web search on and reasoning off; switching web search off
 * takes deep research with it.
 */
function setFeature(feature: Feature, on: boolean) {
  const s = host.settings();
  if (feature === "webSearch") {
    s.chatWebSearch = on;
    if (on) s.chatReasoning = false;
    else s.chatDeepResearch = false;
  } else if (feature === "reasoning") {
    s.chatReasoning = on;
    if (on) {
      s.chatWebSearch = false;
      s.chatDeepResearch = false;
    }
  } else {
    s.chatDeepResearch = on;
    if (on) {
      s.chatWebSearch = true;
      s.chatReasoning = false;
    }
  }
  void host.save();
  drawAccount();
}

/** Drops what the chosen model or the plan no longer allows (as Rust would). */
function sanitizeFeatures(): boolean {
  if (!rd.models && !rd.status) return false;
  const s = host.settings();
  let changed = false;
  if (s.chatDeepResearch && !usable(gate("deepResearch"))) {
    s.chatDeepResearch = false;
    changed = true;
  }
  if (s.chatWebSearch && !usable(gate("webSearch"))) {
    s.chatWebSearch = false;
    s.chatDeepResearch = false;
    changed = true;
  }
  if (s.chatReasoning && !usable(gate("reasoning"))) {
    s.chatReasoning = false;
    changed = true;
  }
  return changed;
}

type Mode = "off" | "web" | "reasoning";

/**
 * Web search and reasoning exclude each other, so they are one choice:
 * Off / Web search / Reasoning. Each option goes through setFeature, so the
 * saved fields and their rules are exactly those of the old pair of switches.
 * Deep research needs web search: while it is on the choice is held at "web".
 */
function modeRow(): HTMLElement {
  const s = host.settings();
  const current: Mode = s.chatReasoning ? "reasoning" : s.chatWebSearch ? "web" : "off";
  const held = s.chatDeepResearch;
  const options: { id: Mode; label: string; feature: Feature | null }[] = [
    { id: "off", label: t("account.modeOff"), feature: null },
    { id: "web", label: t("account.webSearch"), feature: "webSearch" },
    { id: "reasoning", label: t("account.reasoning"), feature: "reasoning" },
  ];
  const enabled = (o: (typeof options)[number]) => !held && (o.feature == null || usable(gate(o.feature)));

  const choose = (id: Mode) => {
    if (id === current) return;
    if (id === "web") setFeature("webSearch", true);
    else if (id === "reasoning") setFeature("reasoning", true);
    else if (current === "web") setFeature("webSearch", false);
    else setFeature("reasoning", false);
    // The section has been redrawn; keep focus on the chosen option.
    queueMicrotask(() => document.getElementById(`rd-mode-${id}`)?.focus({ preventScroll: true }));
  };

  const group = h("div", { class: "segmented", role: "radiogroup", "aria-labelledby": "rd-mode-label" });
  for (const o of options) {
    const on = o.id === current;
    const btn = h("button", {
      type: "button", id: `rd-mode-${o.id}`, role: "radio", class: on ? "seg on" : "seg",
      "aria-checked": on ? "true" : "false", tabindex: on ? "0" : "-1", text: o.label,
    }) as HTMLButtonElement;
    btn.disabled = !enabled(o) && !on;
    btn.addEventListener("click", () => choose(o.id));
    btn.addEventListener("keydown", (e) => {
      const forward = isRtl() ? "ArrowLeft" : "ArrowRight";
      const back = isRtl() ? "ArrowRight" : "ArrowLeft";
      const step = e.key === forward || e.key === "ArrowDown" ? 1 : e.key === back || e.key === "ArrowUp" ? -1 : 0;
      if (!step) return;
      e.preventDefault();
      const usableOpts = options.filter(enabled);
      if (!usableOpts.length) return;
      const i = usableOpts.findIndex((x) => x.id === o.id);
      const next = usableOpts[(i + step + usableOpts.length) % usableOpts.length];
      choose(next.id);
    });
    group.append(btn);
  }

  // Why an option can't be picked: plan lock (with upgrade) or the model.
  const notes = h("div", { class: "mode-notes" });
  if (held) notes.append(h("span", { class: "set-hint", text: t("account.deepHoldsWeb") }));
  for (const o of options) {
    if (!o.feature) continue;
    const g = gate(o.feature);
    if (g.locked) {
      notes.append(h("span", { class: "lock" },
        h("span", { class: "badge" }, icon("lock", 12), h("span", { text: `${o.label}: ${t("account.lockedHint")}` })),
        linkButton(t("account.upgrade"), openSite)));
    } else if (!g.available) {
      notes.append(h("span", { class: "set-hint", text: `${o.label}: ${t("account.notSupported")}` }));
    }
  }

  return h("div", { class: "set-row stack" },
    h("div", { class: "set-text" },
      h("span", { class: "set-label", id: "rd-mode-label", text: t("account.mode") }),
      notes.childElementCount ? notes : null,
    ),
    group,
  );
}

function featuresGroup(): HTMLElement {
  if (sanitizeFeatures()) void host.save();
  const s = host.settings();
  const group = h("div", { class: "card list features", role: "group", "aria-labelledby": "rd-features" },
    h("div", { class: "card-head" },
      h("h3", { id: "rd-features", text: t("account.features") }),
      helpDisclosure(t("account.featuresHint"), `${t("settings.help")}: ${t("account.features")}`),
    ),
  );

  const row = (feature: Feature, label: string, on: boolean, extra?: HTMLElement | null, hint?: string) => {
    const g = gate(feature);
    const sw = switchEl(on && usable(g), !usable(g), label, (v) => setFeature(feature, v));
    const reason = g.locked
      ? h("span", { class: "lock" },
          h("span", { class: "badge" }, icon("lock", 12), h("span", { text: t("account.lockedHint") })),
          linkButton(t("account.upgrade"), openSite))
      : !g.available
        ? h("span", { class: "set-hint", text: t("account.notSupported") })
        : null;
    const el = settingRow({ label, hint, extra: reason, class: usable(g) ? "feature" : "feature off" }, extra ?? null, sw);
    group.append(el);
  };

  group.append(modeRow());

  const effort = h("select", { id: "rd-effort" }) as HTMLSelectElement;
  for (const e of REASONING_EFFORTS) effort.append(h("option", { value: e, text: t(`account.effort.${e}`) }));
  effort.value = s.chatReasoningEffort;
  effort.disabled = !s.chatReasoning || !usable(gate("reasoning"));
  effort.addEventListener("change", () => {
    host.settings().chatReasoningEffort = effort.value as Settings["chatReasoningEffort"];
    void host.save();
  });
  // Effort only matters while reasoning is on; it hangs under the mode row.
  if (s.chatReasoning && usable(gate("reasoning"))) {
    group.append(settingRow({ label: t("account.effort"), forId: "rd-effort", class: "sub" }, effort));
  }
  row("deepResearch", t("account.deepResearch"), s.chatDeepResearch, null,
    !s.chatDeepResearch && s.chatReasoning
      ? `${t("account.deepResearchHint")} ${t("account.excl.offReasoning")}`
      : t("account.deepResearchHint"));
  return group;
}

