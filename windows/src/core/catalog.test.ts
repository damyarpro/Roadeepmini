import { beforeAll, describe, expect, it } from "vitest";
import { catalogEntryOf, catalogOpenUrl, isCategory, isWebUrl, loadCatalog, localized, pillIdOf } from "./catalog";
import { setLanguage } from "./i18n";
import { State } from "./state";
import { hasIntegrationData, renderIntegrationCard, type IntegrationCardHooks } from "../views/integrations";

// Outside Tauri, loadCatalog() serves the built-in preview sample
// (supabase, jira, postmark): enough to drive the pills and the card.
beforeAll(async () => {
  await loadCatalog();
});

describe("catalog helpers", () => {
  it("maps ids to pill ids and back", () => {
    expect(pillIdOf("supabase")).toBe("integration_supabase");
    expect(catalogEntryOf("integration_supabase")?.name).toBe("Supabase");
    expect(catalogEntryOf("integration_stripe")).toBeNull();
    expect(catalogEntryOf("agent:local:supabase")).toBeNull();
  });

  it("knows the ten categories only", () => {
    expect(isCategory("dev")).toBe(true);
    expect(isCategory("commerce")).toBe(true);
    expect(isCategory("code")).toBe(false);
    expect(isCategory(undefined)).toBe(false);
  });

  it("only lets http(s) URLs through", () => {
    expect(isWebUrl("https://example.com/a")).toBe(true);
    expect(isWebUrl("http://localhost:3000")).toBe(true);
    expect(isWebUrl("javascript:alert(1)")).toBe(false);
    expect(isWebUrl("file:///C:/x")).toBe(false);
    expect(isWebUrl("not a url")).toBe(false);
    expect(isWebUrl(42)).toBe(false);
  });

  it("offers a manifest's open page only when it is fixed", () => {
    expect(catalogOpenUrl("integration_supabase")).toBe("https://supabase.com/dashboard/projects");
    // Jira's page depends on the user's site: no fallback before the engine answers.
    expect(catalogOpenUrl("integration_jira")).toBeNull();
    expect(catalogOpenUrl("integration_github")).toBeNull();
  });

  it("picks the text of the current language", () => {
    setLanguage("en");
    expect(localized({ fa: "سلام", en: "Hello" })).toBe("Hello");
    expect(localized({ fa: "سلام", en: "" })).toBe("سلام");
    setLanguage("fa");
    expect(localized({ fa: "سلام", en: "Hello" })).toBe("سلام");
    expect(localized(null)).toBe("");
  });
});

describe("catalog pills", () => {
  it("adds catalog pills from the catalog, after the native ones and before agents", () => {
    State.tasks = [];
    State.settings.activeIntegrations = ["agent:local:abc", "integration_supabase", "integration_github", "integration_unknown"];
    State.loadIntegrationTasks();
    expect(State.tasks.map((t) => t.id)).toEqual([
      "integration_claude", "integration_github", "integration_supabase", "agent:local:abc",
    ]);
    const supabase = State.tasks.find((t) => t.id === "integration_supabase")!;
    expect(supabase).toMatchObject({ name: "Supabase", color: "#3ECF8E", isCatalog: true, isIntegration: true });
  });

  it("drops a catalog pill once it is switched off", () => {
    State.settings.activeIntegrations = ["integration_github"];
    State.loadIntegrationTasks();
    expect(State.tasks.some((t) => t.id === "integration_supabase")).toBe(false);
  });
});

describe("generic list card", () => {
  const hooks: IntegrationCardHooks = {
    detailOpen: false, openDetail() {}, closeDetail() {}, openSettings() {}, openAgent() {},
  };
  const id = "integration_supabase";

  it("renders count, rows and only safe links", () => {
    setLanguage("en");
    State.settings.activeIntegrations = [id];
    State.loadIntegrationTasks();
    const task = State.tasks.find((t) => t.id === id)!;
    State.integrations[id] = {
      loaded: true, error: null, configured: true,
      data: {
        kind: "list", count: 12, more: true, openUrl: null,
        items: [
          { id: "1", title: "prod <b>x</b>", status: "ok", statusText: "ACTIVE_HEALTHY", time: Date.now() - 60_000,
            url: "https://supabase.com/dashboard/project/1" },
          { id: "2", title: "staging", status: "err", url: "javascript:alert(1)" },
          { id: "3", title: "dev", status: "weird" },
          { id: "4", title: "never shown", status: "ok" },
        ],
      },
    };
    expect(hasIntegrationData(id)).toBe(true);
    const card = renderIntegrationCard(task, hooks);
    expect(card.querySelector(".int-total")?.textContent).toBe("12+");
    const rows = card.querySelectorAll(".int-rows > *");
    expect(rows).toHaveLength(3);
    // Text stays text.
    expect(rows[0].querySelector(".int-name")?.textContent).toBe("prod <b>x</b>");
    expect(rows[0].querySelector("b")).toBeNull();
    expect(rows[0].tagName).toBe("BUTTON");
    expect(rows[0].querySelector(".int-badge")?.textContent).toBe("ACTIVE_HEALTHY");
    expect(rows[1].tagName).toBe("DIV");
  });

  it("says when the list is empty", () => {
    const task = State.tasks.find((t) => t.id === id)!;
    State.integrations[id] = {
      loaded: true, error: null, configured: true,
      data: { kind: "list", count: 0, more: false, openUrl: null, items: [] },
    };
    const card = renderIntegrationCard(task, hooks);
    expect(card.querySelector(".int-empty")?.textContent).toBe("Nothing here yet");
  });

  it("falls back to the idle card without data", () => {
    const task = State.tasks.find((t) => t.id === id)!;
    State.integrations[id] = { loaded: false, error: null, configured: false, data: {} };
    expect(hasIntegrationData(id)).toBe(false);
    const card = renderIntegrationCard(task, hooks);
    expect(card.querySelector(".int-status")).not.toBeNull();
  });
});
