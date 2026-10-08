import { describe, expect, it } from "vitest";
import { ActivityStore, normalizeEvent, redactText } from "./store";
import { diffPreview } from "./diff";
import { isTestCommand, parseTestOutput } from "./test-output";
import { recapMarkdown } from "./recap";
import type { CodingEvent } from "./types";

function event(id: string, at: number, extra: Partial<CodingEvent> = {}): CodingEvent {
  return { id, at, sessionId: "one", harness: "codex", kind: "tool", ...extra };
}

describe("coding activity trust boundary", () => {
  it("keeps cancellation terminal without claiming success and reactivates on the next prompt",()=>{
    const store=new ActivityStore();store.ingest(event("start",1,{kind:"prompt"}));store.ingest(event("stop",2,{kind:"cancelled"}));
    expect(store.sessions()[0].status).toBe("cancelled");expect(recapMarkdown(store.sessions()[0],"fa")).toContain("متوقف شد");
    expect(recapMarkdown(store.sessions()[0],"en")).toContain("Status: Stopped");
    store.ingest(event("quota",3,{kind:"usage"}));expect(store.sessions()[0].status).toBe("cancelled");
    store.ingest(event("next",4,{kind:"prompt"}));expect(store.sessions()[0].status).toBe("active");
  });
  it("rejects malformed identities, harnesses, timestamps and fields", () => {
    for (const bad of [null, [], {}, event("", 1), event("a", NaN), event("a", -1), { ...event("a", 1), harness: "other" }]) expect(normalizeEvent(bad)).toBeUndefined();
    expect(normalizeEvent({ ...event("a", 1), exitCode: "0", files: [null, 4, "ok"], command: {} })).toMatchObject({ files: ["ok"] });
    expect(normalizeEvent({ ...event("a", 1), exitCode: "0" })?.exitCode).toBeUndefined();
  });
  it("redacts common secrets before retention and export", () => {
    const secret = "API_KEY=abcdef password='long secret' Authorization: Bearer abcdefghijk sk-123456789abc https://user:pass@example.com";
    const clean = redactText(secret);
    for (const value of ["abcdef", "long secret", "abcdefghijk", "sk-123456789abc", "user:pass"]) expect(clean).not.toContain(value);
    expect(redactText("-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----")).toBe("[redacted private key]");
    const store = new ActivityStore();
    store.ingest(event("a", 1, { kind: "session", title: secret }));
    expect(recapMarkdown(store.sessions()[0])).not.toContain("long secret");
  });
  it("bounds memory, input and output and prevents callers mutating retained data", () => {
    const store = new ActivityStore();
    for (let i = 0; i < 200; i++) store.ingest(event(String(i), i, { output: "a".repeat(13000), files: ["file"] }));
    expect(store.sessions()[0].events).toHaveLength(160);
    expect(store.sessions()[0].events[0].output?.length).toBeLessThan(12100);
    store.sessions()[0].events[0].files!.push("evil");
    expect(store.sessions()[0].events[0].files).toEqual(["file"]);
    for (let i = 0; i < 30; i++) store.ingest(event(`session-${i}`, 500 + i, { sessionId: String(i) }));
    expect(store.sessions()).toHaveLength(16);
  });
});

describe("paired session evidence", () => {
  it("deduplicates replay, notifies subscribers and releases them", () => {
    const store = new ActivityStore(); let notified = 0;
    const stop = store.subscribe(() => notified++);
    expect(store.ingest(event("a", 1))).toBe(true);
    expect(store.ingest(event("a", 1))).toBe(false);
    expect(notified).toBe(1); stop(); store.clear();
    expect(notified).toBe(1); expect(store.sessions()).toEqual([]);
  });
  it("removes only Codex on repeated monitor-off refreshes and retains Claude evidence", () => {
    const store = new ActivityStore(); let notified = 0;
    store.ingest(event("codex", 1));
    store.ingest(event("claude", 2, { harness: "claude", sessionId: "claude-session" }));
    const stop = store.subscribe(() => notified++);
    store.clearHarness("codex");
    expect(store.sessions().map(session => session.harness)).toEqual(["claude"]);
    expect(notified).toBe(1);
    store.clearHarness("codex");
    expect(store.sessions()[0].events[0].id).toBe("claude");
    expect(notified).toBe(1);
    store.clear(); expect(store.sessions()).toEqual([]);
    stop();
  });
  it("pairs late output by call ID and handles snapshot arriving out of order", () => {
    const store = new ActivityStore();
    store.ingest(event("result", 20, { callId: "test", phase: "completed", output: "Tests  8 passed (8)", exitCode: 0 }));
    store.ingest(event("start", 10, { callId: "test", phase: "started", command: "npm test" }));
    expect(store.sessions()[0].test).toMatchObject({ at: 10, verdict: "passed", freshness: "current", passed: 8 });
    expect(store.sessions()[0].counts.tools).toBe(1);
  });
  it("invalidates tests after edits, including edits during the test run", () => {
    const store = new ActivityStore();
    store.ingest(event("start", 10, { callId: "test", phase: "started", command: "cargo test" }));
    store.ingest(event("edit", 15, { tool: "apply_patch", phase: "completed", exitCode: 0, files: ["a.ts"], patch: "+hello" }));
    store.ingest(event("result", 20, { callId: "test", phase: "completed", output: "test result: ok. 4 passed; 0 failed; 0 ignored", exitCode: 0 }));
    expect(store.sessions()[0].test).toMatchObject({ verdict: "passed", freshness: "stale" });
    expect(store.sessions()[0].lastPatch?.preview.added).toBe(1);
  });
  it("does not treat reads as mutations or fabricate Claude output", () => {
    const store = new ActivityStore();
    store.ingest(event("read", 1, { tool: "read_file", files: ["a.ts"] }));
    store.ingest(event("test", 2, { harness: "claude", sessionId: "claude", command: "npm test", phase: "completed" }));
    expect(store.sessions().find(s => s.id === "one")?.changedFiles).toEqual([]);
    expect(store.sessions().find(s => s.id === "claude")?.test?.verdict).toBe("unknown");
  });
  it("accepts Claude successful edit observations without synthesizing command exit codes", () => {
    const store = new ActivityStore();
    store.ingest(event("write", 1, { harness: "claude", tool: "Write", phase: "completed", files: ["file.ts"] }));
    expect(store.sessions()[0].changedFiles).toEqual(["file.ts"]);
    store.ingest(event("test", 2, { harness: "claude", tool: "Bash", command: "npm test", phase: "completed" }));
    expect(store.sessions()[0].test?.verdict).toBe("unknown");
    const text = recapMarkdown(store.sessions()[0], "fa");
    expect(text).toContain("شواهد تست");
    expect(text).toContain("نامشخص");
    expect(text).not.toContain("Exit status is unknown");
  });
  it("reports advisory Windows normalized overlaps only for active rooted sessions", () => {
    const store = new ActivityStore();
    store.ingest(event("a", 1, { phase: "completed", exitCode: 0, cwd: "C:\\Work", tool: "edit_file", files: ["src\\a.ts"] }));
    store.ingest(event("b", 2, { phase: "completed", exitCode: 0, sessionId: "two", cwd: "c:/work/", tool: "write_file", files: ["src/./A.ts"] }));
    expect(store.sessions()[0].conflicts).toEqual([{ file: "src/./A.ts", sessionId: "one" }]);
    store.ingest(event("finish", 3, { sessionId: "two", kind: "finished" }));
    expect(store.sessions().every(s => s.conflicts.length === 0)).toBe(true);
  });
  it("keeps failed edits unconfirmed, inflight edits uncertain and resets each new request", () => {
    const store = new ActivityStore();
    store.ingest(event("test", 1, { command: "npm test", phase: "completed", output: "Tests 2 passed", exitCode: 0 }));
    store.ingest(event("start", 2, { callId: "edit", tool: "apply_patch", phase: "started", files: ["a.ts"], patch: "+x" }));
    expect(store.sessions()[0]).toMatchObject({ changedFiles: [], test: { freshness: "unknown" } });
    store.ingest(event("fail", 3, { callId: "edit", phase: "failed" }));
    expect(store.sessions()[0]).toMatchObject({ changedFiles: [], test: { freshness: "current" } });
    expect(store.sessions()[0].lastPatch).toBeUndefined();
    store.ingest(event("prompt", 4, { kind: "prompt", title: "Next task" }));
    expect(store.sessions()[0].test).toBeUndefined();
    expect(store.sessions()[0].counts).toEqual({ tools: 0, errors: 0 });
    expect(store.sessions()[0].events.map(e => e.id)).toEqual(["prompt"]);
  });
  it("escapes hostile Markdown and gives actionable handoff without shell execution", () => {
    const store = new ActivityStore();
    store.ingest(event("a", 1, { title: "[click](javascript:evil)", kind: "session" }));
    const recap = recapMarkdown(store.sessions()[0]);
    expect(recap).toContain("\\[click\\]");
    expect(recap).toContain("Run the relevant tests");
    expect(recap).toContain("not proof of correctness");
  });
});

describe("test parser evidence semantics", () => {
  it("recognizes common test commands without generic build commands", () => {
    for (const command of ["npm test", "pnpm run test:unit", "cargo test", "python -m pytest", "node --test", "go test ./..."]) expect(isTestCommand(command)).toBe(true);
    for (const command of ["npm run build", "cat tests.ts", "echo test", "npm install", "echo 'npm test'", "cat <<EOF\nnpm test\nEOF"]) expect(isTestCommand(command)).toBe(false);
  });
  it("requires completed nonempty evidence and preserves failure over passing summaries", () => {
    expect(parseTestOutput("Tests  4 passed (4)", 0).verdict).toBe("passed");
    expect(parseTestOutput("Tests  4 passed (4)").verdict).toBe("unknown");
    expect(parseTestOutput("Tests  4 passed (4)\n[truncated]", 0).verdict).toBe("unknown");
    expect(parseTestOutput("0 passed", 0).verdict).toBe("unknown");
    expect(parseTestOutput("Tests  3 skipped (3)", 0).verdict).toBe("skipped");
    expect(parseTestOutput("Tests  4 passed | 1 failed (5)", 0).verdict).toBe("failed");
    expect(parseTestOutput("Tests  4 passed (4)", 2).verdict).toBe("failed");
  });
  it("supports Rust, pytest and Node summaries", () => {
    expect(parseTestOutput("test result: ok. 2 passed; 0 failed; 1 ignored", 0)).toMatchObject({ passed: 2, skipped: 1, verdict: "passed" });
    expect(parseTestOutput("=== 6 passed, 1 skipped in 0.6s ===", 0)).toMatchObject({ passed: 6, skipped: 1 });
    expect(parseTestOutput("# pass 7\n# fail 0\n# skipped 0", 0)).toMatchObject({ passed: 7, verdict: "passed" });
  });
  it("bounds diff previews and excludes metadata from line counts", () => {
    const preview = diffPreview("--- a.ts\n+++ a.ts\n@@ -1 +1 @@\n-old\n+new");
    expect(preview).toMatchObject({ added: 1, removed: 1, truncated: false });
    expect(diffPreview(Array(200).fill("+x").join("\n"))).toMatchObject({ added: 200, truncated: true });
  });
});
