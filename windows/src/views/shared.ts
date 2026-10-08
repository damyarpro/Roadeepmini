// Pieces the island's chat and Create views share, so both read the same:
// the token balance chip, the out-of-credit alert and the history clock.

import { h, svg } from "./dom";
import { formatNumber, t } from "../core/i18n";

/** Stroked clock on the 24 grid (draw with `svg(ISLAND_CLOCK_ICON, 13, { stroke: 1.8 })`). */
export const ISLAND_CLOCK_ICON = "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16z M12 8v4l3 2";

const COIN_ICON =
  "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 2a7 7 0 1 1 0 14 7 7 0 0 1 0-14zm0 3a4 4 0 1 0 0 8 4 4 0 0 0 0-8z";

/** Exact below 10,000, then one decimal with K/M/B (هزار/میلیون/میلیارد) — like the mobile chip. */
export function compactTokens(units: number): string {
  const whole = Math.max(0, Math.floor(units));
  if (whole < 10_000) return formatNumber(whole, { useGrouping: true });
  const steps = [[1e9, "balance.billion"], [1e6, "balance.million"], [1e3, "balance.thousand"]] as const;
  let i = steps.findIndex(([v]) => whole >= v);
  let scaled = Math.round((whole / steps[i][0]) * 10) / 10;
  // 999,960 would read "1000K"; carry it into the next unit.
  if (scaled >= 1000 && i > 0) {
    i -= 1;
    scaled = Math.round((whole / steps[i][0]) * 10) / 10;
  }
  return t(steps[i][1], { n: formatNumber(scaled, { maximumFractionDigits: 1 }) });
}

export interface BalanceValue {
  units: number | null;
  plan: string | null;
}

/**
 * The token balance pill: «۴۲٫۸ هزار توکن» / "42.8K tokens", red and «توکن تمام شد»
 * at zero, the plan in its tooltip, hidden while the balance is unknown. The
 * caller decides what a click does (it is a button).
 */
export function createBalanceChip(): { el: HTMLElement; set(b: BalanceValue | null): void } {
  const text = h("span");
  const el = h("button", { type: "button", class: "bal-chip" }, svg(COIN_ICON, 11), text);
  el.hidden = true;
  let key = "";
  return {
    el,
    set(b) {
      const units = b?.units ?? null;
      const plan = b?.plan?.trim() || null;
      const next = `${t("balance.amount")}|${units}|${plan}`;
      if (next === key) return;
      key = next;
      el.hidden = units == null;
      if (units == null) return;
      const empty = units <= 0;
      el.classList.toggle("empty", empty);
      // A lone Persian zero is a dot: say it in words.
      text.textContent = empty ? t("balance.emptyShort") : t("balance.amount", { n: compactTokens(units) });
      const exact = formatNumber(Math.max(0, Math.floor(units)), { useGrouping: true });
      const tip = empty
        ? t("balance.emptyTip")
        : plan ? t("balance.tip", { n: exact, plan }) : t("balance.tipNoPlan", { n: exact });
      el.title = tip;
      el.setAttribute("aria-label", tip);
    },
  };
}

/** An inline alert in the chat log: a message and the one action that fixes it. */
export function createIslAlert(opts: { text: string; action: string; onAction(): void }): HTMLElement {
  return h("div", { class: "isl-alert", role: "alert" },
    h("span", { class: "isl-alert-text", text: opts.text }),
    h("button", { type: "button", class: "btn primary", text: opts.action, onclick: () => opts.onAction() }),
  );
}

/** The out-of-credit message with its «افزایش اعتبار» button. */
export function createCreditAlert(opts: { onTopUp(): void }): HTMLElement {
  return createIslAlert({ text: t("rerr.INSUFFICIENT_CREDITS"), action: t("balance.topUp"), onAction: opts.onTopUp });
}
