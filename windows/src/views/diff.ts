// Diff card — port of DiffCardView / DiffLineRowView (IslandViewContent.swift).
// Opens in the overview's left card when a diff row of the ticker is clicked.
// Code reads left to right in both languages, so the card's lines do too.

import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { fileName, type DiffKind, type FileDiff } from "../core/diff";
import { t } from "../core/i18n";

const SYMBOLS: Record<DiffKind, string> = { added: "+", removed: "−", context: " " };

export interface DiffCardHooks {
  dismiss(): void;
  /** ↗ — opens the file in the editor; absent when the app cannot do it safely. */
  open?(path: string): void;
}

export function buildDiffCard(diff: FileDiff, hooks: DiffCardHooks): HTMLElement {
  const back = t("common.back");
  const head = h(
    "div",
    { class: "diff-head" },
    h(
      "button",
      { type: "button", class: "diff-back", title: back, "aria-label": back, onclick: () => hooks.dismiss() },
      svg(ICONS.chevronLeft, 9, { stroke: 2.4 }),
      h("b", { dir: "auto", text: fileName(diff.path) }),
    ),
    h("span", { class: "grow" }),
    diff.added > 0 ? h("span", { class: "tick-count plus", text: `+${diff.added}` }) : null,
    diff.removed > 0 ? h("span", { class: "tick-count minus", text: `−${diff.removed}` }) : null,
    hooks.open
      ? h(
        "button",
        {
          type: "button", class: "icon-btn", title: t("live.openFile"), "aria-label": t("live.openFile"),
          onclick: () => hooks.open?.(diff.path),
        },
        svg(ICONS.arrowUpRight, 8),
      )
      : null,
  );

  const lines = diff.hunks.flatMap((hunk) => hunk.lines);
  let content: HTMLElement;
  if (diff.tooLarge) {
    content = h("div", { class: "diff-note", text: t("live.tooLarge") });
  } else if (lines.length === 0) {
    content = h("div", { class: "diff-note", text: t("live.noChanges") });
  } else {
    content = h("div", { class: "diff-lines" });
    const rows = document.createDocumentFragment();
    for (const line of lines) {
      rows.append(
        h(
          "div",
          { class: `diff-line ${line.kind}` },
          h("span", { class: "sym", text: SYMBOLS[line.kind] }),
          h("span", { class: "txt", text: line.text }),
        ),
      );
    }
    content.append(rows);
  }

  return h("div", { class: "diff-card", dir: "ltr" }, head, content);
}
