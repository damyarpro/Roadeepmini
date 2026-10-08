import { describe, expect, it, vi } from "vitest";
import { renderMarkdown, safeUrl } from "./markdown";

function render(src: string, openUrl = vi.fn()) {
  const root = document.createElement("div");
  root.append(renderMarkdown(src, { openUrl, copy: () => Promise.resolve() }));
  return root;
}

describe("blocks", () => {
  it("renders paragraphs with dir=auto and line breaks", () => {
    const root = render("first line\nsecond line\n\nnew paragraph");
    const ps = root.querySelectorAll("p");
    expect(ps).toHaveLength(2);
    expect(ps[0].getAttribute("dir")).toBe("auto");
    expect(ps[0].querySelectorAll("br")).toHaveLength(1);
    expect(ps[1].textContent).toBe("new paragraph");
  });

  it("renders headings # to #### (deeper ones as h4)", () => {
    const root = render("# One\n## Two\n### Three\n#### Four\n###### Six");
    expect(root.querySelector("h1")?.textContent).toBe("One");
    expect(root.querySelector("h2")?.textContent).toBe("Two");
    expect(root.querySelector("h3")?.textContent).toBe("Three");
    expect(root.querySelectorAll("h4")).toHaveLength(2);
    expect(root.querySelector("h1")?.getAttribute("dir")).toBe("auto");
  });

  it("renders a fenced code block as LTR with a copy button", () => {
    const root = render("```ts\nconst a = 1 < 2;\n```\nafter");
    const pre = root.querySelector("pre")!;
    expect(pre.getAttribute("dir")).toBe("ltr");
    expect(pre.textContent).toBe("const a = 1 < 2;");
    expect(root.querySelector(".md-code-lang")?.textContent).toBe("ts");
    expect(root.querySelector("button.md-copy")).not.toBeNull();
    expect(root.querySelector("p")?.textContent).toBe("after");
  });

  it("copies the code and confirms", async () => {
    const copy = vi.fn(() => Promise.resolve());
    const root = document.createElement("div");
    root.append(renderMarkdown("```\nx = 1\n```", { copy }));
    const btn = root.querySelector("button.md-copy") as HTMLButtonElement;
    const before = btn.textContent;
    btn.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(copy).toHaveBeenCalledWith("x = 1");
    expect(btn.textContent).not.toBe(before);
    expect(btn.classList.contains("done")).toBe(true);
  });

  it("renders unordered, ordered and nested lists", () => {
    const root = render("- a\n- b\n  - b1\n  - b2\n- c\n\n3. three\n4. four");
    const ul = root.querySelector("ul")!;
    expect(ul.children).toHaveLength(3);
    const nested = ul.children[1].querySelector("ul")!;
    expect(nested.children).toHaveLength(2);
    expect(nested.children[1].textContent).toBe("b2");
    const ol = [...root.children].find((c) => c.tagName === "OL")!;
    expect(ol.getAttribute("start")).toBe("3");
    expect(ol.children).toHaveLength(2);
  });

  it("renders blockquotes with nested markdown", () => {
    const root = render("> quoted **bold**\n> - item");
    const q = root.querySelector("blockquote")!;
    expect(q.querySelector("strong")?.textContent).toBe("bold");
    expect(q.querySelector("li")?.textContent).toBe("item");
  });

  it("renders horizontal rules", () => {
    const root = render("above\n\n---\n\nbelow");
    expect(root.querySelector("hr")).not.toBeNull();
    expect(root.querySelectorAll("p")).toHaveLength(2);
  });

  it("renders pipe tables inside a scroll wrapper", () => {
    const root = render("| نام | مقدار |\n|---|:---:|\n| a | `x|y` |\n| b |");
    const wrap = root.querySelector(".md-table")!;
    expect(wrap.querySelector("table")).not.toBeNull();
    expect([...wrap.querySelectorAll("th")].map((th) => th.textContent)).toEqual(["نام", "مقدار"]);
    const rows = wrap.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelector("code")?.textContent).toBe("x|y");
    // A short row is padded to the header's width.
    expect(rows[1].querySelectorAll("td")).toHaveLength(2);
  });
});

describe("direction", () => {
  it("gives lists, quotes and tables the direction of their text", () => {
    const root = render("- مورد English\n- دوم\n\n> نقل قول\n\n- English first\n\n| نام | x |\n|---|---|\n| a | b |");
    const lists = root.querySelectorAll("ul");
    expect(lists[0].getAttribute("dir")).toBe("rtl");
    expect(lists[1].getAttribute("dir")).toBe("ltr");
    expect(root.querySelector("blockquote")?.getAttribute("dir")).toBe("rtl");
    expect(root.querySelector("table")?.getAttribute("dir")).toBe("rtl");
    expect(root.querySelector(".md-table")?.getAttribute("dir")).toBe("rtl");
    // Items follow their list, so every marker sits on the same side.
    expect(root.querySelector("li")?.hasAttribute("dir")).toBe(false);
  });
});

describe("inline", () => {
  it("renders bold, italic, strikethrough and inline code", () => {
    const root = render("**b** *i* _u_ ~~s~~ `c`");
    expect(root.querySelector("strong")?.textContent).toBe("b");
    expect([...root.querySelectorAll("em")].map((e) => e.textContent)).toEqual(["i", "u"]);
    expect(root.querySelector("del")?.textContent).toBe("s");
    const code = root.querySelector("code")!;
    expect(code.textContent).toBe("c");
    expect(code.getAttribute("dir")).toBe("ltr");
  });

  it("nests emphasis", () => {
    const root = render("**bold *and italic***");
    const strong = root.querySelector("strong")!;
    expect(strong.querySelector("em")?.textContent).toBe("and italic");
  });

  it("leaves snake_case and lone asterisks alone", () => {
    const root = render("snake_case_name and 2 * 3 * 4");
    expect(root.querySelector("em")).toBeNull();
    expect(root.textContent).toBe("snake_case_name and 2 * 3 * 4");
  });

  it("renders http(s) links that open through openUrl and show the host", () => {
    const openUrl = vi.fn();
    const root = render("see [the docs](https://example.com/a?b=1) now", openUrl);
    const a = root.querySelector("a")!;
    expect(a.querySelector(".md-host")?.textContent).toBe("example.com");
    a.click();
    expect(openUrl).toHaveBeenCalledWith("https://example.com/a?b=1");
  });

  it("links bare and angle-bracket URLs without trailing punctuation", () => {
    const root = render("go to https://roadeep.com. or <http://x.org/p>");
    const links = [...root.querySelectorAll("a")].map((a) => a.getAttribute("href"));
    expect(links).toEqual(["https://roadeep.com/", "http://x.org/p"]);
  });

  it("renders a javascript: link as plain text", () => {
    const root = render("[click me](javascript:alert(1)) and [f](file:///c:/x)");
    expect(root.querySelector("a")).toBeNull();
    expect(root.textContent).toContain("click me");
    expect(root.innerHTML).not.toContain("javascript:");
  });

  it("does not fetch images: they become links", () => {
    const root = render("![logo](https://example.com/x.png)");
    expect(root.querySelector("img")).toBeNull();
    expect(root.querySelector("a")?.textContent).toContain("logo");
  });

  it("shows HTML in the input as text", () => {
    const root = render("<img src=x onerror=alert(1)> <script>alert(2)</script>\n\n<b>hi</b>");
    expect(root.querySelector("img")).toBeNull();
    expect(root.querySelector("script")).toBeNull();
    expect(root.querySelector("b")).toBeNull();
    expect(root.textContent).toContain("<script>alert(2)</script>");
  });

  it("honours backslash escapes", () => {
    const root = render("\\*not italic\\*");
    expect(root.querySelector("em")).toBeNull();
    expect(root.textContent).toBe("*not italic*");
  });
});

describe("streaming tolerance", () => {
  it("runs an unclosed fence to the end", () => {
    const root = render("intro\n```py\nprint('a')\nprint('b')");
    expect(root.querySelector("pre")?.textContent).toBe("print('a')\nprint('b')");
    expect(root.querySelector("p")?.textContent).toBe("intro");
  });

  it("keeps an unclosed bold or code span literal", () => {
    const root = render("this is **half done and `code");
    expect(root.querySelector("strong")).toBeNull();
    expect(root.querySelector("code")).toBeNull();
    expect(root.textContent).toBe("this is **half done and `code");
  });

  it("copes with a half-written table, list and link", () => {
    const root = render("| a | b |\n|--\n- item\n- \n[label](https://exa");
    expect(root.textContent).toContain("item");
    expect(root.querySelector("a")?.getAttribute("href")).toBe("https://exa/");
  });

  it("caps nesting depth", () => {
    const deep = Array.from({ length: 30 }, (_, n) => `${"  ".repeat(n)}- level ${n}`).join("\n");
    const root = render(deep);
    expect(root.textContent).toContain("level 29");
    expect(root.querySelectorAll("ul").length).toBeLessThanOrEqual(8);
  });
});

describe("performance", () => {
  // Openers without closers are the quadratic worst case for an inline parser.
  // "*a `b " and "a ``b " also build thousands of <code> nodes, which happy-dom
  // creates far slower than a browser: for those, linear growth is what counts.
  const time = (src: string) => {
    const t0 = performance.now();
    const root = render(src);
    const ms = performance.now() - t0;
    expect(root.textContent!.length).toBeGreaterThan(0);
    return ms;
  };
  const patterns = ["*a ", "*a `b ", "a ``b ", "**a ", "[a ", "<a ", "_a ", "~~a ", "[a](b "];
  for (const p of patterns) {
    it(`renders 30k chars of ${JSON.stringify(p)} in under 50 ms (or linearly)`, () => {
      const src = p.repeat(Math.ceil(30_000 / p.length));
      time(src.slice(0, 2000)); // warm up
      const half = time(src.slice(0, 15_000));
      const full = time(src);
      expect(full).toBeLessThan(Math.max(50, half * 3));
    });
  }
});

describe("safeUrl", () => {
  it("only accepts http and https", () => {
    expect(safeUrl("https://a.com")?.host).toBe("a.com");
    expect(safeUrl("javascript:alert(1)")).toBeNull();
    expect(safeUrl("data:text/html,x")).toBeNull();
    expect(safeUrl("  HTTP://A.com ")?.protocol).toBe("http:");
  });
});
