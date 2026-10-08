import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { registerMessages, setLanguage } from "../core/i18n";
import { settingsDashboard, settingsDestination, SETTINGS_CATEGORIES, SETTINGS_GROUPS, type SettingsCategory } from "./dashboard";
registerMessages({"assistant.settingsTitle":"Assistant","plannerSettings.title":"Planner","mcpc.title":"MCP servers","shortcuts.title":"Shortcuts"},{"assistant.settingsTitle":"دستیار","plannerSettings.title":"برنامه‌ریز","mcpc.title":"سرورهای MCP","shortcuts.title":"میان‌برها"});

function fixture(initial?: ReturnType<typeof settingsDestination>) {
 const panes = Object.fromEntries(SETTINGS_CATEGORIES.map(item => {
  const pane = document.createElement("section"); pane.id = `sec-${item.id}`;
  const input = document.createElement("input"); input.id = `input-${item.id}`; pane.append(input);
  return [item.id,pane];
 })) as Record<SettingsCategory,HTMLElement>;
 const view = settingsDashboard({panes,initial}); document.body.append(view.element);
 return {view,panes};
}
beforeEach(() => setLanguage("fa"));
afterEach(() => {document.body.replaceChildren();setLanguage("fa");});
describe("settings dashboard navigation", () => {
 it("defaults to discoverable dashboard and makes every retained pane inert", () => {
  const {view,panes}=fixture();expect(view.destination).toBe("dashboard");
  expect(view.element.querySelectorAll("button[data-nav]")).toHaveLength(9);
  for(const pane of Object.values(panes)){expect(pane.hidden).toBe(true);expect(pane.inert).toBe(true);}
 });
 it("validates routes and preserves the Claude deep link", () => {
  expect(settingsDestination(null)).toBe("dashboard");expect(settingsDestination("unknown")).toBe("dashboard");
  expect(settingsDestination("claude")).toBe("mcp");expect(settingsDestination("assistant")).toBe("assistant");
  const {panes}=fixture(settingsDestination("claude"));expect(panes.mcp.hidden).toBe(false);expect(panes.assistant.hidden).toBe(true);
 });
 it("groups every real category once by purpose, without dropping functionality",()=>{
  const {view}=fixture();expect(view.element.querySelectorAll(".settings-category-group")).toHaveLength(3);
  const grouped=SETTINGS_GROUPS.flatMap(group=>[...group.categories]);expect(new Set(grouped).size).toBe(9);
  expect([...grouped].sort()).toEqual(SETTINGS_CATEGORIES.map(item=>item.id).sort());
 });
 it("focuses the existing pane heading instead of repeating its title in the app header",()=>{
  const {view,panes}=fixture();const sectionHead=document.createElement("header");sectionHead.className="sec-head";
  const title=document.createElement("h2");title.textContent="General";title.tabIndex=-1;sectionHead.append(title);panes.general.prepend(sectionHead);
  view.navigate("general");expect(document.activeElement).toBe(title);expect(view.element.querySelector("h1")?.textContent).toBe("تنظیمات");
 });
 it("shows only one category and retains typed values and the same nodes", () => {
  const {view,panes}=fixture();view.navigate("account");const input=panes.account.querySelector("input")!;input.value="unsaved entry";
  view.navigate("assistant");expect(panes.account.hidden).toBe(true);expect(panes.account.inert).toBe(true);expect(panes.assistant.hidden).toBe(false);
  view.navigate("account");expect(panes.account.querySelector("input")).toBe(input);expect(input.value).toBe("unsaved entry");
 });
 it("focuses the destination heading and returns focus to the originating category", () => {
  const {view}=fixture();const card=view.element.querySelector<HTMLButtonElement>('button[data-nav="general"]')!;card.click();
  expect(document.activeElement?.classList.contains("settings-destination-title")).toBe(true);
  view.element.querySelector<HTMLButtonElement>("[data-settings-back]")!.click();expect(view.destination).toBe("dashboard");expect(document.activeElement).toBe(card);
 });
 it("supports keyboard back without consuming ordinary editing keys", () => {
  const {view}=fixture("general");view.element.dispatchEvent(new KeyboardEvent("keydown",{key:"ArrowLeft",bubbles:true}));expect(view.destination).toBe("general");
  view.element.dispatchEvent(new KeyboardEvent("keydown",{key:"ArrowLeft",altKey:true,bubbles:true}));expect(view.destination).toBe("dashboard");
 });
 it("restores each retained pane's independent scroll position", () => {
  const {view}=fixture("general");const scroll=view.element.querySelector<HTMLElement>("main")!;scroll.scrollTop=240;
  view.navigate("assistant");expect(scroll.scrollTop).toBe(0);scroll.scrollTop=80;
  view.navigate("general");expect(scroll.scrollTop).toBe(240);view.navigate("assistant");expect(scroll.scrollTop).toBe(80);
 });
 it("retains the selected destination when language rebuilding refreshes labels", () => {
  const old=fixture("assistant");const selected=old.view.destination;old.view.element.remove();setLanguage("en");
  const fresh=fixture(selected);expect(fresh.view.destination).toBe("assistant");expect(fresh.panes.assistant.hidden).toBe(false);
  expect(fresh.view.element.querySelector("[data-settings-back]")?.textContent).toContain("All settings");
 });
});
