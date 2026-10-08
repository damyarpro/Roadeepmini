// The catalog: services described by a bundled JSON manifest and run by the
// generic engine in src-tauri/src/catalog (no per-service code). Both windows
// read it through `catalog_list`; the 12 hand-coded ("native") services are not
// in it.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI } from "./bridge";
import { getLanguage } from "./i18n";

export interface Localized {
  fa: string;
  en: string;
}

/** The market's categories, in the order they are listed. */
export const CATEGORIES = [
  "payments", "dev", "monitoring", "work", "comms", "automation", "commerce", "support", "marketing", "content",
] as const;

export type Category = (typeof CATEGORIES)[number];

export const isCategory = (c: unknown): c is Category => CATEGORIES.includes(c as Category);

export interface CatalogField {
  /** Credential Manager key, "x.<id>.<name>". */
  key: string;
  name: string;
  /** "secret" is never read back; "text" and "url" are. */
  kind: "secret" | "text" | "url";
  label: Localized;
  placeholder: string;
  optional: boolean;
  help: Localized | null;
}

/** One `catalog_list` entry (lib.rs). */
export interface CatalogEntry {
  id: string;
  name: string;
  category: string;
  color: string;
  desc: Localized;
  keyUrl: string;
  docsUrl: string | null;
  openUrl: string | null;
  /** Where requests may go; may hold "*.example.com" or "{field.<name>}". */
  hosts: string[];
  pollEvery: number;
  fields: CatalogField[];
}

// Local copy of state.ts isHexColor: state.ts imports this module.
const isHexColor = (c: unknown) => typeof c === "string" && /^#[0-9A-Fa-f]{6}$/.test(c);

/** Pill / task id of a service, native or catalog. */
export const pillIdOf = (id: string) => `integration_${id}`;

/** The text for the current language, English when the Persian one is missing. */
export function localized(text: Localized | null | undefined): string {
  if (!text) return "";
  return (getLanguage() === "fa" ? text.fa : text.en) || text.en || text.fa || "";
}

/** Only http(s) pages are ever handed to the browser. */
export function isWebUrl(url: unknown): url is string {
  if (typeof url !== "string") return false;
  try {
    const u = new URL(url);
    return u.protocol === "https:" || u.protocol === "http:";
  } catch {
    return false;
  }
}

// Plain-browser preview only (`npm run dev`), so the market and the generic
// card can be looked at without the app. Never shipped data: IS_TAURI is true there.
const PREVIEW_SAMPLE: CatalogEntry[] = [
  {
    id: "supabase", name: "Supabase", category: "dev", color: "#3ECF8E",
    desc: { fa: "پروژه‌های Supabase و وضعیت سلامتشان", en: "Your Supabase projects and their health" },
    keyUrl: "https://supabase.com/dashboard/account/tokens", docsUrl: "https://supabase.com/docs/reference/api",
    openUrl: "https://supabase.com/dashboard/projects", hosts: ["api.supabase.com"], pollEvery: 120,
    fields: [{
      key: "x.supabase.token", name: "token", kind: "secret",
      label: { fa: "توکن دسترسی", en: "Access token" }, placeholder: "sbp_…", optional: false,
      help: { fa: "در داشبورد: Account → Access Tokens", en: "Dashboard: Account → Access Tokens" },
    }],
  },
  {
    id: "jira", name: "Jira", category: "work", color: "#2684FF",
    desc: { fa: "کارهایی که در Jira به تو سپرده شده", en: "Jira issues assigned to you" },
    keyUrl: "https://id.atlassian.com/manage-profile/security/api-tokens", docsUrl: null,
    openUrl: null, hosts: ["{field.site}"], pollEvery: 180,
    fields: [
      { key: "x.jira.site", name: "site", kind: "url", label: { fa: "آدرس سایت", en: "Site URL" },
        placeholder: "https://your-team.atlassian.net", optional: false, help: null },
      { key: "x.jira.email", name: "email", kind: "text", label: { fa: "ایمیل", en: "Email" },
        placeholder: "you@example.com", optional: false, help: null },
      { key: "x.jira.token", name: "token", kind: "secret", label: { fa: "توکن API", en: "API token" },
        placeholder: "", optional: false, help: null },
    ],
  },
  {
    id: "postmark", name: "Postmark", category: "comms", color: "#FFDE00",
    desc: { fa: "ایمیل‌های ارسالی اخیر و وضعیت تحویلشان", en: "Recently sent emails and how they were delivered" },
    keyUrl: "https://account.postmarkapp.com/servers", docsUrl: null, openUrl: "https://account.postmarkapp.com",
    hosts: ["api.postmarkapp.com"], pollEvery: 120,
    fields: [{ key: "x.postmark.token", name: "token", kind: "secret", label: { fa: "توکن سرور", en: "Server token" },
      placeholder: "", optional: false, help: null }],
  },
];

let cache: Promise<CatalogEntry[]> | null = null;
let loaded: CatalogEntry[] = [];

/**
 * The catalog, read once per window (it is bundled into the app, so it can't
 * change while it runs). Empty when the command fails; the failure is logged.
 */
export function loadCatalog(): Promise<CatalogEntry[]> {
  cache ??= (async () => {
    if (!IS_TAURI) return PREVIEW_SAMPLE;
    try {
      const list = await invoke<CatalogEntry[]>("catalog_list");
      return Array.isArray(list) ? list : [];
    } catch (err) {
      void Bridge.log(`catalog: catalog_list failed: ${String(err)}`);
      return [];
    }
  })().then((list) => {
    // Colours end up in style attributes; anything but #RRGGBB is dropped.
    loaded = list.filter((e) => isHexColor(e.color));
    if (loaded.length !== list.length) void Bridge.log(`catalog: skipped ${list.length - loaded.length} entr(ies) with a bad colour`);
    return loaded;
  });
  return cache;
}

/** What has been loaded so far (empty before loadCatalog resolves). */
export const catalogNow = (): readonly CatalogEntry[] => loaded;

/** The catalog entry behind a pill id, if it is a catalog service. */
export function catalogEntryOf(pillId: string): CatalogEntry | null {
  if (!pillId.startsWith("integration_")) return null;
  const id = pillId.slice("integration_".length);
  return loaded.find((e) => e.id === id) ?? null;
}

/**
 * The page a catalog service's header link opens before any data arrived:
 * only a fixed http(s) URL, since a templated one needs the engine to fill it.
 */
export function catalogOpenUrl(pillId: string): string | null {
  const url = catalogEntryOf(pillId)?.openUrl;
  return url && !url.includes("{") && isWebUrl(url) ? url : null;
}
