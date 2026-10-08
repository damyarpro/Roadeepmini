import type { DiffPreview } from "./types";

/** Compact evidence only: a patch is never interpreted as an instruction. */
export function diffPreview(patch: string): DiffPreview {
  const raw = patch.split(/\r?\n/);
  let added = 0;
  let removed = 0;
  const lines: DiffPreview["lines"] = [];
  for (const text of raw) {
    const header = /^(?:diff |index |@@|--- |\+\+\+ |\*\*\*)/.test(text);
    const kind = header ? "header" : text.startsWith("+") ? "added" : text.startsWith("-") ? "removed" : "context";
    if (kind === "added") added++;
    if (kind === "removed") removed++;
    if (lines.length < 100) lines.push({ kind, text: text.slice(0, 400) });
  }
  return { lines, added, removed, truncated: raw.length > 100 || raw.some(line => line.length > 400) || patch.includes("[truncated]") };
}
