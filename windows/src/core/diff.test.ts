// Live diff engine (core/diff.ts) — port of the macOS DiffEngine tests, plus
// the tool-input glue (buildFileDiff) the macOS HookServer does.

import { describe, expect, it } from "vitest";
import {
  DIFF_MAX_LINES, buildFileDiff, fileName, fromEdit, fromNew, isDiffStep, lastTextStep,
  makeDiffStep, parseDiffStep, toOneLine, type FileDiff,
} from "./diff";

const allLines = (d: FileDiff) => d.hunks.flatMap((h) => h.lines);

describe("fromEdit / fromNew", () => {
  it("counts additions", () => {
    const d = fromEdit("", "hello\nworld\n", "/a/b.swift");
    expect([d.added, d.removed, d.tooLarge, d.hunks.length]).toEqual([2, 0, false, 1]);
    expect(fileName(d.path)).toBe("b.swift");
  });

  it("counts removals", () => {
    const d = fromEdit("hello\nworld\n", "", "/x.py");
    expect([d.added, d.removed, d.hunks.length]).toEqual([0, 2, 1]);
  });

  it("keeps context lines around a replacement", () => {
    const d = fromEdit("foo\nbar\nbaz\n", "foo\nqux\nbaz\n", "/f.ts");
    expect([d.added, d.removed]).toEqual([1, 1]);
    expect(d.hunks[0].lines.map((l) => [l.kind, l.text, l.origLine, l.newLine])).toEqual([
      ["context", "foo", 1, 1],
      ["removed", "bar", 2, -1],
      ["added", "qux", -1, 2],
      ["context", "baz", 3, 3],
    ]);
  });

  it("marks every line of a new file as added", () => {
    const d = fromNew("line1\nline2\nline3\n", "/new.rs");
    expect([d.added, d.removed, d.isNewFile]).toEqual([3, 0, true]);
    expect(allLines(d).every((l) => l.kind === "added")).toBe(true);
  });

  it("keeps only the counts past 200 KB", () => {
    const d = fromEdit("x".repeat(150 * 1024), "y".repeat(60 * 1024), "/big.swift");
    expect(d.tooLarge).toBe(true);
    expect(d.hunks).toEqual([]);
    expect([d.added, d.removed]).toEqual([1, 1]);
  });

  it("guards 200 KB in UTF-8 bytes, not characters", () => {
    expect(fromEdit("é".repeat(60 * 1024), "è".repeat(60 * 1024), "/accents.txt").tooLarge).toBe(true);
  });

  it("normalises CRLF and copes with no trailing newline", () => {
    const crlf = fromEdit("a\r\nb\r\n", "a\r\nc\r\n", "/win.txt");
    expect([crlf.added, crlf.removed]).toEqual([1, 1]);
    const bare = fromEdit("hello", "hello\nworld", "/t.txt");
    expect([bare.added, bare.removed]).toEqual([1, 0]);
  });

  it("puts 3 lines of context around a change, and separates distant changes", () => {
    const lines = Array.from({ length: 10 }, (_, i) => `line${i + 1}`);
    const changed = lines.map((l, i) => (i === 4 ? "changed" : l));
    const d = fromEdit(`${lines.join("\n")}\n`, `${changed.join("\n")}\n`, "/ctx.swift");
    expect(d.hunks).toHaveLength(1);
    expect(d.hunks[0].lines.filter((l) => l.kind === "context")).toHaveLength(6);
    expect([d.hunks[0].origStart, d.hunks[0].newStart]).toEqual([2, 2]);

    const many = Array.from({ length: 30 }, (_, i) => `l${i}`);
    const two = many.map((l, i) => (i === 2 || i === 25 ? `${l}!` : l));
    const far = fromEdit(many.join("\n"), two.join("\n"), "/two.ts");
    expect([far.hunks.length, far.added, far.removed]).toEqual([2, 2, 2]);
  });

  it("stops before the quadratic table (m·n > 1 000 000)", () => {
    const many = Array.from({ length: 1001 }, (_, i) => `line${i}`).join("\n");
    const d = fromEdit(many, `${many}\nextra`, "/big.swift");
    expect(d.tooLarge).toBe(true);
    expect(d.hunks).toEqual([]);
  });

  it("keeps the line count of a new file too large to show", () => {
    const d = fromNew("x\n".repeat(DIFF_MAX_LINES + 1), "/new.swift");
    expect([d.tooLarge, d.isNewFile, d.hunks.length]).toEqual([true, true, 0]);
    expect(d.added).toBeGreaterThan(0);
  });
});

describe("diff steps", () => {
  it("round-trips through the step encoding", () => {
    const s = makeDiffStep("foo.swift", 3, 1, 7);
    expect(isDiffStep(s)).toBe(true);
    expect(parseDiffStep(s)).toEqual({ filename: "foo.swift", added: 3, removed: 1, diffId: 7 });
    expect(parseDiffStep(makeDiffStep("bar.ts", 0, 2, 42))?.diffId).toBe(42);
  });

  it("never reads an ordinary or malformed step as a diff", () => {
    expect(isDiffStep("Edit foo.swift")).toBe(false);
    expect(parseDiffStep("Edit foo.swift")).toBeNull();
    expect(parseDiffStep("\uE001no-tab")).toBeNull();
    expect(parseDiffStep("\uE001f\t1:2")).toBeNull();
    expect(parseDiffStep("\uE001f\t1:2:x")).toBeNull();
  });

  it("finds the last step that is text", () => {
    expect(lastTextStep(["Reads · a.ts", makeDiffStep("a.ts", 1, 0, 1)])).toBe("Reads · a.ts");
    expect(lastTextStep([makeDiffStep("a.ts", 1, 0, 1)])).toBeUndefined();
    expect(lastTextStep([])).toBeUndefined();
  });
});

describe("toOneLine", () => {
  it("keeps the first paragraph as plain text", () => {
    expect(toOneLine("line one\nline two\nline three")).toBe("line one line two line three");
    expect(toOneLine("**hello** world")).toBe("hello world");
    expect(toOneLine("## My Title\nsome text")).toBe("My Title some text");
    expect(toOneLine("")).toBe("");
    expect(toOneLine("First para.\n\nSecond para.")).toBe("First para.");
    expect(toOneLine("Done. Commit 450a657.\n\n---\n\nFiles touched (7)…")).toBe("Done. Commit 450a657.");
    expect(toOneLine("Summary line.\n***\nMore details.")).toBe("Summary line.");
    expect(toOneLine("Result:\n| Col1 | Col2 |\n|---|---|\n| A | B |")).toBe("Result:");
    expect(toOneLine("\n\nActual content.")).toBe("Actual content.");
    expect(toOneLine("Fixed `it`.\r\n\r\nDetails")).toBe("Fixed it.");
  });

  it("drops list markers", () => {
    expect(toOneLine("- item one\n- item two")).toBe("item one item two");
    expect(toOneLine("* first\n* second")).toBe("first second");
    expect(toOneLine("1. step one\n2. step two")).toBe("step one step two");
  });

  it("caps the length in characters, Persian included", () => {
    expect(toOneLine("x ".repeat(200), 10).length).toBeLessThanOrEqual(10);
    expect(Array.from(toOneLine("سلام ".repeat(100), 12))).toHaveLength(12);
  });
});

describe("buildFileDiff", () => {
  it("diffs an Edit, and nothing that changes nothing", () => {
    const d = buildFileDiff("Edit", { file_path: "C:\\p\\a.ts", old_string: "a\nb", new_string: "a\nc\nd" })!;
    expect(fileName(d.path)).toBe("a.ts");
    expect([d.added, d.removed, d.isNewFile]).toEqual([2, 1, false]);
    expect(buildFileDiff("Edit", { file_path: "/a", old_string: "x", new_string: "x" })).toBeNull();
    expect(buildFileDiff("Edit", { file_path: "/a", old_string: "", new_string: "" })).toBeNull();
    expect(buildFileDiff("Edit", { old_string: "x", new_string: "y" })).toBeNull();
    expect(buildFileDiff("Edit", { file_path: "/a", old_string: 1, new_string: "y" })).toBeNull();
  });

  it("sums the edits of a MultiEdit", () => {
    const d = buildFileDiff("MultiEdit", {
      file_path: "/p/m.kt",
      edits: [
        { old_string: "aaa", new_string: "bbb" },
        { old_string: "ccc", new_string: "ddd\neee" },
        { old_string: 3 },
        null,
      ],
    })!;
    expect([d.added, d.removed, d.hunks.length]).toEqual([3, 2, 2]);
    expect(buildFileDiff("MultiEdit", { file_path: "/p/m.kt", edits: [] })).toBeNull();
    expect(buildFileDiff("MultiEdit", { file_path: "/p/m.kt" })).toBeNull();
  });

  it("diffs a Write as a new file, and ignores other tools", () => {
    const d = buildFileDiff("Write", { file_path: "/p/n.md", content: "# Hi\n\nThere\n" })!;
    expect([d.added, d.removed, d.isNewFile]).toEqual([3, 0, true]);
    expect(buildFileDiff("Write", { file_path: "/p/n.md", content: "" })).toBeNull();
    expect(buildFileDiff("Bash", { command: "ls" })).toBeNull();
    expect(buildFileDiff("Read", { file_path: "/p/n.md" })).toBeNull();
  });
});
