// The island's waiting cards (permission, question), the diff card in the
// overview, and the finished card's final line.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { State, type AgentTask } from "../core/state";
import { setLanguage, t } from "../core/i18n";
import { makeDiffStep, fromEdit } from "../core/diff";
import type { AskedQuestion } from "../core/bridge-hooks";
import { beginApproval, endApproval } from "../island/approvals";
import { appendSessionDiff, clearSessionTrail, setFinalLine } from "../island/live-session";
import {
  CLICK_GUARD_MS, buildApproval, buildFinished, buildOverview, buildQuestion, type ViewActions,
} from "./views";

const actions = () => ({
  setView: vi.fn(), collapse: vi.fn(), foldApproval: vi.fn(), setFocus: vi.fn(), openTerminal: vi.fn(),
  openTarget: vi.fn(), openUrl: vi.fn(), decide: vi.fn(), answer: vi.fn(), answerInTerminal: vi.fn(),
  toggleSound: vi.fn(), setVolume: vi.fn(), setAutoClose: vi.fn(), openSettingsWindow: vi.fn(), blip: vi.fn(),
  toggleMaximize: vi.fn(),
} satisfies ViewActions);

const CLAUDE = "integration_claude";
const claude = (extra: Partial<AgentTask> = {}): AgentTask => ({
  id: CLAUDE, name: "Claude Code · app", color: "#F5F6F8", state: "working", steps: [], stepIndex: 0,
  source: "claudeCode", isIntegration: true, ...extra,
});

let now = 1000;
const advance = (ms: number) => { now += ms; };
const buttons = (el: HTMLElement) => [...el.querySelectorAll<HTMLButtonElement>(".actions button")];
const labelled = (el: HTMLElement, text: string) => buttons(el).find((b) => b.textContent?.includes(text))!;

const QUESTIONS: AskedQuestion[] = [
  { question: "Which database?", header: "DB", multiSelect: false, options: [{ label: "Postgres", description: "SQL" }, { label: "SQLite", description: "" }] },
  { question: "Extras?", header: "", multiSelect: true, options: [{ label: "Tests", description: "" }, { label: "Docs", description: "" }, { label: "CI", description: "" }] },
];

beforeEach(() => {
  setLanguage("en");
  now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  endApproval();
  clearSessionTrail(CLAUDE);
  State.tasks = [claude()];
  State.focusId = CLAUDE;
  State.pendingApproval = null;
  State.view = "overview";
  State.mode = "expanded";
});
afterEach(() => vi.restoreAllMocks());

describe("permission card", () => {
  it("ignores clicks for its first 600 ms, then takes them", () => {
    const a = actions();
    const view = buildApproval(a);
    beginApproval({ taskId: CLAUDE, requestId: "r1", sessionId: "s", tool: "Bash", command: "Bash · rm -rf build" }, null);
    view.sync();
    labelled(view.el, "Allow").click();
    advance(CLICK_GUARD_MS - 1);
    labelled(view.el, "Deny").click();
    expect(a.decide).not.toHaveBeenCalled();
    advance(1);
    labelled(view.el, "Allow").click();
    expect(a.decide).toHaveBeenCalledWith("allow");
    expect(view.el.querySelector(".code")?.textContent).toBe("Bash · rm -rf build");
  });

  it("guards each new request again", () => {
    const a = actions();
    const view = buildApproval(a);
    beginApproval({ taskId: CLAUDE, requestId: "r1", sessionId: "s", tool: "Bash", command: "Bash · ls" }, null);
    view.sync();
    advance(CLICK_GUARD_MS);
    endApproval();
    beginApproval({ taskId: CLAUDE, requestId: "r2", sessionId: "s", tool: "Bash", command: "Bash · ls" }, null);
    view.sync();
    labelled(view.el, "Allow").click();
    expect(a.decide).not.toHaveBeenCalled();
  });

  it("folds away without answering, and only while a request waits", () => {
    const a = actions();
    const view = buildApproval(a);
    view.sync();
    const fold = view.el.querySelector<HTMLButtonElement>(".fold")!;
    expect(fold.style.display).toBe("none");
    beginApproval({ taskId: CLAUDE, requestId: "r1", sessionId: "s", tool: "Bash", command: "Bash · ls" }, null);
    view.sync();
    expect(fold.style.display).toBe("");
    expect(fold.getAttribute("aria-label")).toBe(t("card.fold"));
    fold.click();
    expect(a.foldApproval).toHaveBeenCalledOnce();
    expect(a.decide).not.toHaveBeenCalled();
  });
});

describe("question card", () => {
  const open = () => {
    const a = actions();
    const view = buildQuestion(a);
    beginApproval({ taskId: CLAUDE, requestId: "q1", sessionId: "s", tool: "AskUserQuestion", command: "AskUserQuestion" }, QUESTIONS);
    view.sync();
    advance(CLICK_GUARD_MS);
    return { a, view };
  };

  it("steps through the questions and answers by option position", () => {
    const { a, view } = open();
    expect(view.el.querySelector(".title")?.textContent).toBe("Which database?");
    expect(view.el.querySelector(".who-row")?.textContent).toContain("is asking (1 of 2)");
    expect(labelled(view.el, "Postgres").title).toBe("SQL");
    labelled(view.el, "SQLite").click();
    view.sync();
    expect(view.el.querySelector(".title")?.textContent).toBe("Extras?");
    expect(view.el.querySelector(".who-row")?.textContent).toContain("is asking (2 of 2)");

    // Pick several: toggles, then Done (off until something is picked).
    const done = () => labelled(view.el, "Done");
    expect(done().classList.contains("off")).toBe(true);
    done().click();
    expect(a.answer).not.toHaveBeenCalled();
    labelled(view.el, "CI").click();
    view.sync();
    labelled(view.el, "Tests").click();
    view.sync();
    expect(labelled(view.el, "Tests").getAttribute("aria-pressed")).toBe("true");
    labelled(view.el, "Docs").click();
    view.sync();
    labelled(view.el, "Docs").click(); // and off again
    view.sync();
    done().click();
    expect(a.answer).toHaveBeenCalledWith([1, [0, 2]]);
  });

  it("ignores a click that lands on a fresh question card", () => {
    const a = actions();
    const view = buildQuestion(a);
    beginApproval({ taskId: CLAUDE, requestId: "q2", sessionId: "s", tool: "AskUserQuestion", command: "" }, [QUESTIONS[0]]);
    view.sync();
    labelled(view.el, "Postgres").click();
    expect(a.answer).not.toHaveBeenCalled();
    advance(CLICK_GUARD_MS);
    labelled(view.el, "Postgres").click();
    expect(a.answer).toHaveBeenCalledWith([0]);
  });

  it("hands the question back to the terminal on request", () => {
    const { a, view } = open();
    view.el.querySelector<HTMLButtonElement>(".link-btn")!.click();
    expect(a.answerInTerminal).toHaveBeenCalledOnce();
    expect(a.answer).not.toHaveBeenCalled();
  });

  it("points a notification question to the terminal", () => {
    State.tasks = [claude({ state: "question", steps: ["Shall I continue?"], stepIndex: 0 })];
    const view = buildQuestion(actions());
    view.sync();
    expect(view.el.querySelector(".title")?.textContent).toBe("Shall I continue?");
    expect(view.el.textContent).toContain(t("ask.answerItInTerminal"));
    expect(view.el.querySelector<HTMLElement>(".fold")!.style.display).toBe("none");
  });
});

describe("diff card", () => {
  it("opens from a diff row of the ticker, and Escape or leaving the overview closes it", () => {
    const id = appendSessionDiff(CLAUDE, fromEdit("a\nb\n", "a\nc\n", "D:\\app\\src\\main.ts"));
    State.tasks = [claude({ steps: ["Edits · main.ts", makeDiffStep("main.ts", 1, 1, id)], stepIndex: 1 })];
    const a = actions();
    const view = buildOverview(a);
    document.body.append(view.el);
    try {
      view.sync();
      (view.el.querySelectorAll<HTMLElement>(".ticker-row")[1]).click();
      expect(a.blip).toHaveBeenCalled();
      view.sync();
      const card = view.el.querySelector(".diff-card")!;
      expect(card.querySelector(".diff-back b")?.textContent).toBe("main.ts");
      expect([...card.querySelectorAll(".diff-line")].map((l) => l.className)).toEqual([
        "diff-line context", "diff-line removed", "diff-line added",
      ]);
      expect(view.el.querySelector<HTMLElement>(".jump")!.style.display).toBe("none");

      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
      view.sync();
      expect(view.el.querySelector(".diff-card")).toBeNull();
      expect(view.el.querySelector(".ticker")).not.toBeNull();

      (view.el.querySelectorAll<HTMLElement>(".ticker-row")[1]).click();
      State.view = "prompt";
      State.notify();
      State.view = "overview";
      view.sync();
      expect(view.el.querySelector(".diff-card")).toBeNull();
    } finally {
      view.el.remove();
    }
  });

  it("closes a diff that has since been forgotten", () => {
    const id = appendSessionDiff(CLAUDE, fromEdit("x", "y", "/p/a.ts"));
    State.tasks = [claude({ steps: [makeDiffStep("a.ts", 1, 1, id)], stepIndex: 0 })];
    const view = buildOverview(actions());
    view.sync();
    view.el.querySelector<HTMLElement>(".ticker-row:nth-child(2)")!.click();
    clearSessionTrail(CLAUDE);
    view.sync();
    expect(view.el.querySelector(".diff-card")).toBeNull();
  });
});

describe("finished card and agents' wording", () => {
  it("shows the turn's final message on one line", () => {
    State.tasks = [claude({ state: "finished", steps: ["Runs · tests", makeDiffStep("a.ts", 1, 0, 1)], stepIndex: 1 })];
    const view = buildFinished(actions());
    view.sync();
    const title = view.el.querySelector<HTMLElement>(".title")!;
    // No final line: the last step that is not a diff.
    expect(title.textContent).toBe("Runs · tests");
    setFinalLine(CLAUDE, "Done. Fixed the parser.");
    view.sync();
    expect(title.textContent).toBe("Done. Fixed the parser.");
    expect(title.classList.contains("one-line")).toBe(true);
    expect(view.el.querySelector(".who-row")?.textContent).toContain(t("finished.label"));
  });

  it("names an agent once: its pill's name, then what happened", () => {
    State.tasks = [{ ...claude({ state: "finished" }), id: "integration_gemini", name: "Gemini CLI · app", source: "agent" }];
    State.focusId = "integration_gemini";
    const finished = buildFinished(actions());
    finished.sync();
    expect(finished.el.querySelector(".who-row")?.textContent).toBe(`Gemini CLI · app${t("card.agentFinished")}`);

    State.tasks[0].state = "working";
    State.tasks[0].steps = ["Runs · npm test"];
    const overview = buildOverview(actions());
    overview.sync();
    expect(overview.el.querySelector(".ticker")).not.toBeNull();
    expect(overview.el.querySelector(".who .tool")?.textContent).toBe(t("card.agent"));
  });
});
