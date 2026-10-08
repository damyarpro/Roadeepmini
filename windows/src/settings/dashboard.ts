import { getLanguage, t } from "../core/i18n";
import { h, svg } from "../views/dom";
import { helpDisclosure, icon, type IconName } from "./ui";
import "./dashboard.css";

export const SETTINGS_CATEGORIES = [
  { id: "account", icon: "account", label: "account.title", fa: "ورود و مدیریت حساب رودیپ", en: "Sign in and manage your Roadeep account" },
  { id: "assistant", icon: "agents", label: "assistant.settingsTitle", fa: "مغز داخلی، صدا و دستیار", en: "Local brain, voice and assistant" },
  { id: "agents", icon: "agents", label: "agents.title", fa: "ساخت و مدیریت دستیارها", en: "Create and manage assistants" },
  { id: "planner", icon: "planner", label: "plannerSettings.title", fa: "برنامه‌ها و یادآوری‌ها", en: "Plans and reminders" },
  { id: "integrations", icon: "integrations", label: "integrations.title", fa: "اتصال سرویس‌ها به اپ", en: "Connect services to the app" },
  { id: "mcp", icon: "mcp", label: "nav.mcpClaude", fa: "اتصال ابزارهای کدنویسی و MCP", en: "Connect coding apps and MCP tools" },
  { id: "mcpServers", icon: "servers", label: "mcpc.title", fa: "مدیریت سرورهای ابزار", en: "Manage tool servers" },
  { id: "general", icon: "general", label: "general.title", fa: "ظاهر، زبان و رفتار اپ", en: "Appearance, language and app behavior" },
] as const satisfies readonly { id: string; icon: IconName; label: string; fa: string; en: string }[];
export type SettingsCategory = typeof SETTINGS_CATEGORIES[number]["id"];
export type SettingsDestination = SettingsCategory | "dashboard";
export const SETTINGS_GROUPS = [
  {id:"personal",fa:"حساب و دستیارها",en:"Account and assistants",categories:["account","assistant","agents"]},
  {id:"connections",fa:"اتصال‌ها و ابزارها",en:"Connections and tools",categories:["integrations","mcp","mcpServers"]},
  {id:"preferences",fa:"برنامه و تنظیمات اپ",en:"Plans and app preferences",categories:["planner","general"]},
] as const satisfies readonly {id:string;fa:string;en:string;categories:readonly SettingsCategory[]}[];
export const categoryAnchor = (id: SettingsCategory) => `sec-${id}`;
const copy = (fa: string, en: string) => getLanguage() === "fa" ? fa : en;

export function settingsDestination(value: string | null | undefined): SettingsDestination {
  if (value === "claude") return "mcp";
  return SETTINGS_CATEGORIES.some(item => item.id === value) ? value as SettingsCategory : "dashboard";
}

/** Only navigation owns visibility; section builders keep their native lifecycle. */
export function settingsDashboard(options: {
  panes: Record<SettingsCategory, HTMLElement>;
  initial?: SettingsDestination;
  version?: string;
  footer?: HTMLElement;
  onNavigate?: (destination: SettingsDestination) => void;
}) {
  let destination = options.initial ?? "dashboard";
  const scrollPositions = new Map<SettingsDestination, number>();
  const content = h("main", { class: "content settings-stage" });
  const heading = h("h1", { class: "settings-destination-title", tabindex: "-1" });
  const back = h("button", { type: "button", class: "settings-back", "data-settings-back": "true" },
    icon("arrowBack", 18), h("span", { text: copy("همهٔ تنظیمات", "All settings") })) as HTMLButtonElement;
  const header = h("header", { class: "settings-dashboard-header" }, back,
    h("div", { class: "settings-heading" }, h("span", { class: "settings-app-label", text: t("settings.appName") }), heading),
    options.version ? h("span", { class: "version", dir: "ltr", text: `v${options.version.replace(/^v/i, "")}` }) : null);
  const dashboardTitle = h("h2", { class: "settings-overview-title", tabindex: "-1", text: copy("تنظیمات اپ", "App settings") });
  const groups = h("div", { class: "settings-category-groups" });
  const dashboard = h("section", { class: "settings-overview", "aria-labelledby": "settings-overview-title" }, dashboardTitle, groups, options.footer ?? null);
  dashboardTitle.id = "settings-overview-title";
  const cards = new Map<SettingsCategory, HTMLButtonElement>();
  for (const group of SETTINGS_GROUPS) {
   const titleId = `settings-group-${group.id}`;
   const grid = h("div", { class: "settings-category-grid" });
   groups.append(h("section", {class:"settings-category-group","aria-labelledby":titleId},h("h3",{id:titleId,class:"settings-group-title",text:copy(group.fa,group.en)}),grid));
   for (const category of group.categories) {
    const item = SETTINGS_CATEGORIES.find(item=>item.id===category)!;
    const title = t(item.label);
    const button = h("button", { type: "button", class: "settings-category", "data-nav": item.id, "aria-controls": categoryAnchor(item.id) },
      h("span", { class: "settings-category-icon" }, item.id === "assistant"
        ? svg("M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z M19 10v2a7 7 0 0 1-14 0v-2 M12 19v3 M8 22h8",24,{stroke:1.75})
        : icon(item.icon, 24)),
      h("span", { class: "settings-category-title", text: title })) as HTMLButtonElement;
    button.addEventListener("click", () => navigate(item.id));
    cards.set(item.id, button);
    grid.append(h("article", { class: "settings-category-tile" }, button,
      helpDisclosure(copy(item.fa, item.en), copy(`راهنمای ${title}`, `Help for ${title}`))));
   }
  }
  const paneHost = h("div", { class: "settings-pane-host" }, ...SETTINGS_CATEGORIES.map(item => options.panes[item.id]));
  content.append(header, dashboard, paneHost);
  const element = h("div", { class: "settings-dashboard-shell" }, content);

  function navigate(next: SettingsDestination, focus = true) {
    scrollPositions.set(destination, content.scrollTop);
    const previous = destination;
    destination = next;
    const overview = next === "dashboard";
    dashboard.hidden = !overview;
    dashboard.inert = !overview;
    paneHost.hidden = overview;
    back.hidden = overview;
    heading.textContent = copy("تنظیمات", "Settings");
    for (const item of SETTINGS_CATEGORIES) {
      const pane = options.panes[item.id];
      pane.hidden = item.id !== next;
      pane.inert = item.id !== next;
      pane.setAttribute("aria-hidden", String(item.id !== next));
    }
    content.scrollTop = scrollPositions.get(next) ?? 0;
    if (focus) {
      const target = overview && previous !== "dashboard" ? cards.get(previous) : !overview ? options.panes[next as SettingsCategory].querySelector<HTMLElement>(".sec-head h2") ?? heading : heading;
      target?.focus({ preventScroll: true });
    }
    options.onNavigate?.(next);
  }
  back.addEventListener("click", () => navigate("dashboard"));
  element.addEventListener("keydown", event => {
    if (event.key === "ArrowLeft" && event.altKey && destination !== "dashboard") {
      event.preventDefault(); navigate("dashboard");
    }
  });
  navigate(destination, false);
  return { element, navigate, get destination() { return destination; } };
}
