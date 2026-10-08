// A small Markdown renderer for the island chat's replies. It builds DOM nodes
// directly — the model's text only ever reaches the page through textContent,
// so HTML in a reply shows as text and nothing in it can run.
//
// It is tolerant on purpose: while a reply streams in, a fence or a ** may not
// be closed yet. An open fence runs to the end of the text; an open emphasis
// marker stays literal until its partner arrives.

import { Bridge } from "../core/bridge";
import { t, textDirection } from "../core/i18n";

export interface MarkdownOptions {
  /** Opens a link the user clicked (http/https only ever reach this). */
  openUrl?: (url: string) => void;
  /** Writes a code block to the clipboard. */
  copy?: (text: string) => Promise<void>;
}

/** Lists and quotes deeper than this render their content as paragraphs. */
const MAX_DEPTH = 6;
const COPIED_MS = 1500;

interface Ctx {
  openUrl: (url: string) => void;
  copy: (text: string) => Promise<void>;
}

function defaultCopy(text: string): Promise<void> {
  if (!navigator.clipboard) return Promise.reject(new Error("clipboard unavailable"));
  return navigator.clipboard.writeText(text);
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  return node;
}

/** Renders `source` into a fragment of block elements. */
export function renderMarkdown(source: string, options: MarkdownOptions = {}): DocumentFragment {
  const ctx: Ctx = {
    openUrl: options.openUrl ?? ((url) => void Bridge.openUrl(url)),
    copy: options.copy ?? defaultCopy,
  };
  const frag = document.createDocumentFragment();
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  for (const node of parseBlocks(lines, ctx, 0)) frag.append(node);
  return frag;
}

// ── Blocks ────────────────────────────────────────────────────────────────────

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)/;
const HEADING = /^ {0,3}(#{1,6})(?:\s+(.*?))?\s*#*\s*$/;
const HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}> ?(.*)$/;
const LIST_ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:[ \t]+(.*))?$/;
const TABLE_SEP = /^ *\|? *:?-+:? *(?:\| *:?-+:? *)*\|? *$/;

function indentOf(line: string): number {
  const m = /^ */.exec(line.replace(/\t/g, "    "));
  return m ? m[0].length : 0;
}

/**
 * dir="auto" skips descendants that carry their own dir, so a container whose
 * children all do (a list of items, a quote of paragraphs) would always come
 * out LTR. Containers take the direction of their first strong letter instead.
 */
function containerDir(node: HTMLElement) {
  const dir = textDirection(node.textContent ?? "");
  if (dir) node.dir = dir;
}

function isBlank(line: string): boolean {
  return line.trim() === "";
}

function isTableStart(lines: string[], i: number): boolean {
  const sep = lines[i + 1];
  return sep != null && lines[i].includes("|") && sep.includes("|") && TABLE_SEP.test(sep);
}

/** True when `line` begins a block other than a paragraph (it ends a paragraph). */
function startsBlock(lines: string[], i: number): boolean {
  const line = lines[i];
  return (
    FENCE.test(line) || HEADING.test(line) || HR.test(line) || QUOTE.test(line) ||
    LIST_ITEM.test(line) || isTableStart(lines, i)
  );
}

function parseBlocks(lines: string[], ctx: Ctx, depth: number): HTMLElement[] {
  const out: HTMLElement[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      i++;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1];
      const body: string[] = [];
      i++;
      // An unclosed fence (a reply still streaming) runs to the end.
      while (i < lines.length) {
        const close = lines[i].trim();
        if (close[0] === marker[0] && close.length >= marker.length && /^(`+|~+)$/.test(close)) {
          i++;
          break;
        }
        body.push(lines[i]);
        i++;
      }
      out.push(codeBlock(body.join("\n"), fence[2], ctx));
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const level = Math.min(4, heading[1].length);
      const node = el(`h${level}` as "h1", "md-h");
      node.dir = "auto";
      appendInline(node, heading[2] ?? "", ctx);
      out.push(node);
      i++;
      continue;
    }

    if (HR.test(line)) {
      out.push(el("hr", "md-hr"));
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const inner: string[] = [];
      while (i < lines.length && !isBlank(lines[i])) {
        const m = QUOTE.exec(lines[i]);
        // Lazy continuation: a plain line right after a quote line stays in it.
        inner.push(m ? m[1] : lines[i]);
        i++;
      }
      const node = el("blockquote", "md-quote");
      if (depth < MAX_DEPTH) node.append(...parseBlocks(inner, ctx, depth + 1));
      else appendParagraph(node, inner, ctx);
      containerDir(node);
      out.push(node);
      continue;
    }

    if (LIST_ITEM.test(line)) {
      const { node, next } = parseList(lines, i, ctx, depth);
      out.push(node);
      i = next;
      continue;
    }

    if (isTableStart(lines, i)) {
      const rows: string[] = [lines[i]];
      i += 2;
      while (i < lines.length && !isBlank(lines[i]) && lines[i].includes("|")) {
        rows.push(lines[i]);
        i++;
      }
      out.push(table(rows, ctx));
      continue;
    }

    const para: string[] = [line];
    i++;
    while (i < lines.length && !isBlank(lines[i]) && !startsBlock(lines, i)) {
      para.push(lines[i]);
      i++;
    }
    const p = el("p", "md-p");
    p.dir = "auto";
    appendParagraph(p, para, ctx);
    out.push(p);
  }
  return out;
}

/** Lines joined with <br>: chat replies use single newlines on purpose. */
function appendParagraph(parent: HTMLElement, lines: string[], ctx: Ctx) {
  lines.forEach((line, n) => {
    if (n > 0) parent.append(el("br"));
    appendInline(parent, line.trim(), ctx);
  });
}

function parseList(
  lines: string[], start: number, ctx: Ctx, depth: number,
): { node: HTMLElement; next: number } {
  const first = LIST_ITEM.exec(lines[start])!;
  const baseIndent = indentOf(lines[start]);
  const ordered = /\d/.test(first[2]);
  const list = el(ordered ? "ol" : "ul", "md-list");
  if (ordered) {
    const n = parseInt(first[2], 10);
    if (n !== 1) list.setAttribute("start", String(n));
  }

  let i = start;
  while (i < lines.length) {
    const m = LIST_ITEM.exec(lines[i]);
    if (!m || indentOf(lines[i]) !== baseIndent || /\d/.test(m[2]) !== ordered) break;
    // Content lines of this item: indented deeper, or lazy plain lines.
    const contentIndent = baseIndent + m[2].length + 1;
    const body: string[] = [m[3] ?? ""];
    i++;
    while (i < lines.length) {
      const line = lines[i];
      if (isBlank(line)) {
        // A blank line only continues the item when indented content follows.
        const next = lines[i + 1];
        if (next != null && !isBlank(next) && indentOf(next) > baseIndent) {
          body.push("");
          i++;
          continue;
        }
        break;
      }
      const ind = indentOf(line);
      if (ind > baseIndent) {
        body.push(line.replace(/\t/g, "    ").slice(Math.min(ind, contentIndent)));
        i++;
        continue;
      }
      if (LIST_ITEM.test(line) || startsBlock(lines, i)) break;
      body.push(line.trim());
      i++;
    }
    const li = el("li", "md-li");
    const blocks = depth < MAX_DEPTH ? parseBlocks(body, ctx, depth + 1) : [];
    if (depth >= MAX_DEPTH) appendParagraph(li, body, ctx);
    else if (blocks.length === 1 && blocks[0].tagName === "P") {
      // Tight item: its text sits in the <li> itself.
      li.append(...Array.from(blocks[0].childNodes));
    } else li.append(...blocks);
    list.append(li);
    // Skip blank lines between items of the same list.
    let j = i;
    while (j < lines.length && isBlank(lines[j])) j++;
    if (j > i && j < lines.length) {
      const nm = LIST_ITEM.exec(lines[j]);
      if (nm && indentOf(lines[j]) === baseIndent && /\d/.test(nm[2]) === ordered) i = j;
    }
  }
  containerDir(list);
  return { node: list, next: i };
}

function splitRow(row: string): string[] {
  let s = row.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  const cells: string[] = [];
  let cur = "";
  let inCode = false;
  for (let k = 0; k < s.length; k++) {
    const ch = s[k];
    if (ch === "\\" && s[k + 1] === "|") {
      cur += "|";
      k++;
    } else if (ch === "`") {
      inCode = !inCode;
      cur += ch;
    } else if (ch === "|" && !inCode) {
      cells.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  cells.push(cur.trim());
  return cells;
}

function table(rows: string[], ctx: Ctx): HTMLElement {
  const wrap = el("div", "md-table");
  const tbl = el("table");
  const head = splitRow(rows[0]);
  const thead = el("thead");
  const htr = el("tr");
  for (const cell of head) {
    const th = el("th");
    appendInline(th, cell, ctx);
    htr.append(th);
  }
  thead.append(htr);
  const tbody = el("tbody");
  for (const row of rows.slice(1)) {
    const tr = el("tr");
    const cells = splitRow(row);
    for (let c = 0; c < head.length; c++) {
      const td = el("td");
      appendInline(td, cells[c] ?? "", ctx);
      tr.append(td);
    }
    tbody.append(tr);
  }
  tbl.append(thead, tbody);
  // Cells follow the table's direction (from its header); each cell's own
  // text still lays out by its content (unicode-bidi: plaintext in the CSS).
  containerDir(thead);
  if (thead.dir) tbl.dir = wrap.dir = thead.dir;
  thead.removeAttribute("dir");
  wrap.append(tbl);
  return wrap;
}

function codeBlock(text: string, lang: string, ctx: Ctx): HTMLElement {
  const wrap = el("div", "md-code");
  wrap.dir = "ltr";
  const head = el("div", "md-code-head");
  const label = el("span", "md-code-lang");
  label.textContent = lang;
  const copy = el("button", "md-copy");
  copy.type = "button";
  copy.textContent = t("md.copy");
  copy.setAttribute("aria-label", t("md.copyCode"));
  let reset: number | null = null;
  copy.addEventListener("click", () => {
    ctx.copy(text).then(
      () => {
        copy.textContent = t("md.copied");
        copy.classList.add("done");
        if (reset != null) window.clearTimeout(reset);
        reset = window.setTimeout(() => {
          reset = null;
          copy.textContent = t("md.copy");
          copy.classList.remove("done");
        }, COPIED_MS);
      },
      (err) => {
        void Bridge.log(`chat: copy failed ${String(err)}`);
        copy.textContent = t("md.copyFailed");
      },
    );
  });
  head.append(label, copy);
  const pre = el("pre");
  pre.dir = "ltr";
  const code = el("code");
  code.textContent = text;
  pre.append(code);
  wrap.append(head, pre);
  return wrap;
}

// ── Inline ────────────────────────────────────────────────────────────────────

const ESCAPABLE = /[\\`*_{}[\]()#+\-.!|~<>]/;
/** Sticky: matched at `lastIndex`, so no slice is allocated per position. */
const BARE_URL = /https?:\/\/[^\s<>"'`]+/iy;
/** A link label longer than this is not looked for (keeps "[[[[…" linear). */
const MAX_LABEL = 2000;

/**
 * Searches that already failed in this run of text. A search that found
 * nothing from position p finds nothing from any later position either, so
 * each kind of search scans the text at most once: long replies full of
 * unmatched `*`, backticks or "<" stay linear instead of quadratic.
 */
interface Memo {
  /** Delimiter → smallest start from which no closer exists. */
  noCloser: Map<string, number>;
  /** Backtick run length → smallest start from which no such run exists. */
  noTicks: Map<number, number>;
  noGt: number;
  noParen: number;
  /** "[" index → its matching "]" (or -1), found in one pass on first use. */
  brackets: Map<number, number> | null;
}

const newMemo = (): Memo => ({
  noCloser: new Map(), noTicks: new Map(), noGt: Infinity, noParen: Infinity, brackets: null,
});

/** Pairs every "[" with its "]" in one stack pass (escaped ones excluded). */
function pairBrackets(s: string): Map<number, number> {
  const pairs = new Map<number, number>();
  const stack: number[] = [];
  for (let k = 0; k < s.length; k++) {
    const c = s[k];
    if (c === "\\") k++;
    else if (c === "[") stack.push(k);
    else if (c === "]" && stack.length) pairs.set(stack.pop()!, k);
  }
  for (const open of stack) pairs.set(open, -1);
  return pairs;
}

function tickRun(s: string, k: number): number {
  let n = 0;
  while (s.charCodeAt(k + n) === 96) n++;
  return n;
}

/** Start of the next run of `len` backticks at or after `from`, or -1. */
function findTicks(s: string, from: number, len: number, memo: Memo): number {
  if ((memo.noTicks.get(len) ?? Infinity) <= from) return -1;
  const end = s.indexOf("`".repeat(len), from);
  if (end < 0) memo.noTicks.set(len, Math.min(memo.noTicks.get(len) ?? Infinity, from));
  return end;
}

/** Index of the next `ch` at or after `from`, remembering a miss in `memo[key]`. */
function findChar(s: string, from: number, ch: string, memo: Memo, key: "noGt" | "noParen"): number {
  if (memo[key] <= from) return -1;
  const at = s.indexOf(ch, from);
  if (at < 0) memo[key] = Math.min(memo[key], from);
  return at;
}

/** The URL when it is http(s) and parses, else null. */
export function safeUrl(raw: string): URL | null {
  const s = raw.trim();
  if (!/^https?:\/\//i.test(s)) return null;
  try {
    const u = new URL(s);
    return u.protocol === "http:" || u.protocol === "https:" ? u : null;
  } catch {
    return null;
  }
}

function link(url: URL, label: string | null, ctx: Ctx): HTMLElement {
  const a = el("a", "md-link");
  a.href = url.href;
  a.rel = "noopener noreferrer";
  a.title = url.href;
  const open = (e: Event) => {
    e.preventDefault();
    ctx.openUrl(url.href);
  };
  a.addEventListener("click", open);
  a.addEventListener("auxclick", (e) => e.preventDefault());
  if (label != null) {
    appendInline(a, label, ctx, true);
    const host = el("span", "md-host");
    host.textContent = url.host;
    host.dir = "ltr";
    a.append(host);
  } else {
    a.textContent = url.href;
    a.dir = "ltr";
  }
  return a;
}

/** Finds the end of `[label]` starting at `open` (a "["), honouring nesting. */
function closeBracket(s: string, open: number, memo: Memo): number {
  memo.brackets ??= pairBrackets(s);
  const close = memo.brackets.get(open) ?? -1;
  return close > 0 && close - open <= MAX_LABEL ? close : -1;
}

/** Index of the closing `delim`, or -1. The closer must follow a non-space. */
function findCloser(s: string, from: number, delim: string, memo: Memo): number {
  if ((memo.noCloser.get(delim) ?? Infinity) <= from) return -1;
  const found = scanCloser(s, from, delim, memo);
  if (found < 0) memo.noCloser.set(delim, Math.min(memo.noCloser.get(delim) ?? Infinity, from));
  return found;
}

function scanCloser(s: string, from: number, delim: string, memo: Memo): number {
  let k = from;
  while (k < s.length) {
    if (s[k] === "\\") {
      k += 2;
      continue;
    }
    if (s[k] === "`") {
      // Skip code spans: their content is not emphasis.
      const run = tickRun(s, k);
      const end = findTicks(s, k + run, run, memo);
      k = end < 0 ? k + run : end + run;
      continue;
    }
    if (s.startsWith(delim, k) && k > from && !/\s/.test(s[k - 1])) {
      // A single * must not be part of a ** run.
      if (delim.length === 1 && (s[k + 1] === delim || s[k - 1] === delim)) {
        k++;
        continue;
      }
      if (delim === "_" && /[\p{L}\p{N}]/u.test(s[k + 1] ?? "")) {
        k++;
        continue;
      }
      // "***" closing "**…*…": the inner * takes the first one.
      while (delim.length === 2 && delim !== "~~" && s[k + 2] === delim[0]) k++;
      return k;
    }
    k++;
  }
  return -1;
}

function appendInline(parent: HTMLElement, s: string, ctx: Ctx, inLink = false) {
  const memo = newMemo();
  let text = "";
  const flush = () => {
    if (text) parent.append(document.createTextNode(text));
    text = "";
  };
  let k = 0;
  while (k < s.length) {
    const ch = s[k];

    if (ch === "\\" && k + 1 < s.length && ESCAPABLE.test(s[k + 1])) {
      text += s[k + 1];
      k += 2;
      continue;
    }

    if (ch === "`") {
      const run = tickRun(s, k);
      const end = findTicks(s, k + run, run, memo);
      if (end > 0) {
        flush();
        const code = el("code", "md-ic");
        code.dir = "ltr";
        let body = s.slice(k + run, end);
        if (/^ .* $/.test(body)) body = body.slice(1, -1);
        code.textContent = body;
        parent.append(code);
        k = end + run;
        continue;
      }
      text += s.slice(k, k + run);
      k += run;
      continue;
    }

    // ~~strike~~, **bold** / __bold__, *em* / _em_
    const strong = s.startsWith("**", k) ? "**" : s.startsWith("__", k) ? "__" : null;
    const delim = s.startsWith("~~", k) ? "~~" : strong ?? (ch === "*" || ch === "_" ? ch : null);
    if (delim) {
      const opensWord = delim[0] !== "_" || k === 0 || !/[\p{L}\p{N}]/u.test(s[k - 1]);
      const nextCh = s[k + delim.length];
      if (opensWord && nextCh && !/\s/.test(nextCh)) {
        const end = findCloser(s, k + delim.length, delim, memo);
        if (end > 0) {
          flush();
          const tag = delim === "~~" ? "del" : delim.length === 2 ? "strong" : "em";
          const node = el(tag);
          appendInline(node, s.slice(k + delim.length, end), ctx, inLink);
          parent.append(node);
          k = end + delim.length;
          continue;
        }
      }
      text += delim;
      k += delim.length;
      continue;
    }

    // [label](url) and ![alt](url) — images become links: no remote fetches.
    const image = ch === "!" && s[k + 1] === "[";
    if ((ch === "[" || image) && !inLink) {
      const open = image ? k + 1 : k;
      const close = closeBracket(s, open, memo);
      if (close > 0 && s[close + 1] === "(") {
        const paren = findChar(s, close + 2, ")", memo, "noParen");
        if (paren > 0) {
          const label = s.slice(open + 1, close);
          const target = s.slice(close + 2, paren).trim().split(/\s+/)[0]?.replace(/^<|>$/g, "") ?? "";
          flush();
          const url = safeUrl(target);
          // Anything but http(s) (javascript:, file:, data:…) is just its label.
          if (url) parent.append(link(url, label || url.host, ctx));
          else appendInline(parent, label, ctx, inLink);
          k = paren + 1;
          continue;
        }
      }
    }

    // <https://…> and bare URLs.
    if (!inLink && ch === "<") {
      const end = findChar(s, k, ">", memo, "noGt");
      const url = end > 0 ? safeUrl(s.slice(k + 1, end)) : null;
      if (url) {
        flush();
        parent.append(link(url, null, ctx));
        k = end + 1;
        continue;
      }
    }
    if (!inLink && (ch === "h" || ch === "H") && (k === 0 || !/[\p{L}\p{N}]/u.test(s[k - 1]))) {
      BARE_URL.lastIndex = k;
      const m = BARE_URL.exec(s);
      if (m) {
        // Trailing punctuation belongs to the sentence, not the URL.
        let raw = m[0].replace(/[.,;:!?،؛]+$/, "");
        if (raw.endsWith(")") && !raw.includes("(")) raw = raw.slice(0, -1);
        const url = safeUrl(raw);
        if (url) {
          flush();
          parent.append(link(url, null, ctx));
          k += raw.length;
          continue;
        }
      }
    }

    text += ch;
    k++;
  }
  flush();
}
