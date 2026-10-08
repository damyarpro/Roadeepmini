import { afterEach, describe, expect, it, vi } from "vitest";
import { openMarket, type MarketItem } from "./market";
import { setLanguage } from "../core/i18n";
import "../core/error-text"; // registers the integration strings

const ITEMS: MarketItem[] = [
  { id: "integration_stripe", name: "Stripe", color: "#0570DE", category: "payments", desc: "Balance and recent payments" },
  { id: "integration_supabase", name: "Supabase", color: "#3ECF8E", category: "dev", desc: "Your projects" },
  { id: "integration_github", name: "GitHub", color: "#F4505E", category: "dev", desc: "Stars" },
];

const visible = () =>
  [...document.querySelectorAll<HTMLElement>(".mk-card")].filter((c) => !c.hidden).map((c) => c.querySelector(".mk-name")?.textContent);

afterEach(() => {
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  document.body.innerHTML = "";
});

describe("market", () => {
  it("lists every service by name, with added ones marked", () => {
    setLanguage("en");
    openMarket({ items: ITEMS, isAdded: (id) => id === "integration_github", onAdd: vi.fn() });
    expect(document.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
    expect(visible()).toEqual(["GitHub", "Stripe", "Supabase"]);
    const github = [...document.querySelectorAll(".mk-card")].find((c) => c.textContent?.includes("GitHub"))!;
    expect(github.querySelector("button")).toBeNull();
    expect(github.textContent).toContain("Added");
  });

  it("filters by category and by search (name, description or category)", () => {
    openMarket({ items: ITEMS, isAdded: () => false, onAdd: vi.fn() });
    const chips = [...document.querySelectorAll<HTMLButtonElement>(".mk-cat")];
    // All + the two categories present.
    expect(chips.map((c) => c.textContent)).toEqual(["All", "Payments", "Code & deploys"]);
    chips[2].click();
    expect(chips[2].getAttribute("aria-pressed")).toBe("true");
    expect(visible()).toEqual(["GitHub", "Supabase"]);
    chips[0].click();
    const search = document.querySelector<HTMLInputElement>(".mk-search")!;
    search.value = "PAYMENTS";
    search.dispatchEvent(new Event("input"));
    expect(visible()).toEqual(["Stripe"]);
    search.value = "nothing like it";
    search.dispatchEvent(new Event("input"));
    expect(visible()).toEqual([]);
    expect(document.querySelector(".mk-none")?.hasAttribute("hidden")).toBe(false);
  });

  it("closes on Escape and hands an added service over", () => {
    const onAdd = vi.fn();
    openMarket({ items: ITEMS, isAdded: () => false, onAdd });
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".mk")).toBeNull();
    expect(onAdd).not.toHaveBeenCalled();

    openMarket({ items: ITEMS, isAdded: () => false, onAdd });
    const stripe = [...document.querySelectorAll(".mk-card")].find((c) => c.textContent?.includes("Stripe"))!;
    stripe.querySelector<HTMLButtonElement>("button")!.click();
    expect(onAdd).toHaveBeenCalledWith("integration_stripe");
    expect(document.querySelector(".mk")).toBeNull();
  });

  it("folds Arabic letter variants in the search", () => {
    setLanguage("fa");
    openMarket({
      items: [{ id: "integration_x", name: "X", color: "#000000", category: "work", desc: "کارهای یکی" }],
      isAdded: () => false, onAdd: vi.fn(),
    });
    const search = document.querySelector<HTMLInputElement>(".mk-search")!;
    search.value = "كارهاي";
    search.dispatchEvent(new Event("input"));
    expect(visible()).toEqual(["X"]);
  });
});
