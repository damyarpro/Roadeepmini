// The pills next to the character: integrations and agents share MAX_ACTIVE_PILLS slots
// in settings.activeIntegrations. Every switch for a pill goes through here so
// the count, the note and the other switches stay in step.

import { MAX_ACTIVE_PILLS, type Settings } from "../core/state";
import { t } from "../core/i18n";
import { Bridge } from "../core/bridge";
import { switchEl } from "./ui";

export interface PillsHost {
  settings: () => Settings;
  save: () => Promise<void>;
}

let host: PillsHost;
/** Keyed, so a section redrawn on a language change replaces its listener. */
const listeners = new Map<string, () => void>();

export function initPills(h: PillsHost) {
  host = h;
}

export const pillsUsed = () => host.settings().activeIntegrations.length;
export const pillsFull = () => pillsUsed() >= MAX_ACTIVE_PILLS;
export const isPillOn = (id: string) => host.settings().activeIntegrations.includes(id);

/** Called after any pill is switched. */
export function onPillsChange(key: string, fn: () => void) {
  listeners.set(key, fn);
}

/** Forgets the listeners of rows that a redraw removed (a service taken out of the list). */
export function dropPillsListeners(prefix: string) {
  for (const key of [...listeners.keys()]) if (key.startsWith(prefix)) listeners.delete(key);
}

function changed() {
  syncSwitches();
  for (const fn of listeners.values()) fn();
}

/** The list changed elsewhere (the island dropped a deleted agent's pill). */
export const refreshPills = changed;

/** Turns a pill on or off; false when every slot is taken. */
export function setPill(id: string, on: boolean): boolean {
  const s = host.settings();
  if (on === s.activeIntegrations.includes(id)) return true;
  if (on && pillsFull()) return false;
  s.activeIntegrations = on ? [...s.activeIntegrations, id] : s.activeIntegrations.filter((x) => x !== id);
  void host.save();
  void Bridge.log(`settings: pill ${on ? "on" : "off"} (${s.activeIntegrations.length}/${MAX_ACTIVE_PILLS})`);
  changed();
  return true;
}

/** A switch that is off can't be turned on while all slots are taken. */
function syncSwitch(el: HTMLButtonElement) {
  const on = isPillOn(el.dataset.pill ?? "");
  el.classList.toggle("on", on);
  el.setAttribute("aria-checked", on ? "true" : "false");
  const blocked = !on && pillsFull();
  el.disabled = blocked;
  el.title = blocked ? t("pills.full", { max: MAX_ACTIVE_PILLS }) : (el.getAttribute("aria-label") ?? "");
}

function syncSwitches() {
  for (const el of document.querySelectorAll<HTMLButtonElement>("button[data-pill]")) syncSwitch(el);
}

export function pillSwitch(id: string, label: string): HTMLButtonElement {
  const el = switchEl(isPillOn(id), false, label, (on) => {
    if (!setPill(id, on)) syncSwitch(el);
  });
  el.dataset.pill = id;
  syncSwitch(el);
  return el;
}
