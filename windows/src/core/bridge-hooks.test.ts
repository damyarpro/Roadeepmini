import { describe, expect, it } from "vitest";
import { answersFit, askedQuestions, HooksBridge, MAX_OPTIONS, MAX_QUESTIONS, type AskedQuestion } from "./bridge-hooks";

const opts = (...labels: string[]) => labels.map((label) => ({ label, description: `about ${label}` }));

describe("askedQuestions", () => {
  it("reads the questions the relay forwarded for AskUserQuestion", () => {
    const input = {
      questions: [
        { question: "Which one?", header: "Pick", multiSelect: false, options: opts("A", "B") },
        { question: "Extras?", multiSelect: true, options: [{ label: "Tests" }, { label: "Docs" }] },
      ],
    };
    expect(askedQuestions("AskUserQuestion", input)).toEqual([
      { question: "Which one?", header: "Pick", multiSelect: false, options: opts("A", "B") },
      {
        question: "Extras?",
        header: "",
        multiSelect: true,
        options: [
          { label: "Tests", description: "" },
          { label: "Docs", description: "" },
        ],
      },
    ]);
  });

  it("is null for anything the island cannot offer whole", () => {
    const one = { question: "Q?", options: opts("A", "B") };
    expect(askedQuestions("Bash", { questions: [one] })).toBeNull();
    expect(askedQuestions("AskUserQuestion", undefined)).toBeNull();
    expect(askedQuestions("AskUserQuestion", { command: "ls" })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: [] })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: Array(MAX_QUESTIONS + 1).fill(one) })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: [{ question: "", options: opts("A", "B") }] })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: [{ question: "Q?", options: opts("A") }] })).toBeNull();
    const many = opts(...Array.from({ length: MAX_OPTIONS + 1 }, (_, i) => `o${i}`));
    expect(askedQuestions("AskUserQuestion", { questions: [{ question: "Q?", options: many }] })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: [{ question: "Q?", options: [{ label: "A" }, { label: 2 }] }] })).toBeNull();
    expect(askedQuestions("AskUserQuestion", { questions: [null] })).toBeNull();
  });
});

describe("answersFit", () => {
  const qs: AskedQuestion[] = [
    { question: "Which one?", header: "", multiSelect: false, options: opts("A", "B") },
    { question: "Extras?", header: "", multiSelect: true, options: opts("Tests", "Docs", "Lint") },
  ];

  it("takes one answer per question, by position", () => {
    expect(answersFit(qs, [1, [0, 2]])).toBe(true);
    expect(answersFit(qs, [0, [1]])).toBe(true);
  });

  it("refuses wrong counts, shapes and indexes", () => {
    expect(answersFit(qs, [1])).toBe(false);
    expect(answersFit(qs, [1, [0], 0])).toBe(false);
    expect(answersFit(qs, [2, [0]])).toBe(false);
    expect(answersFit(qs, [-1, [0]])).toBe(false);
    expect(answersFit(qs, [0.5, [0]])).toBe(false);
    expect(answersFit(qs, [[0], [0]])).toBe(false);
    expect(answersFit(qs, [0, 1])).toBe(false);
    expect(answersFit(qs, [0, []])).toBe(false);
    expect(answersFit(qs, [0, [1, 1]])).toBe(false);
    expect(answersFit(qs, [0, [3]])).toBe(false);
    expect(answersFit([], [])).toBe(false);
  });
});

describe("HooksBridge", () => {
  it("answers nothing outside Tauri", async () => {
    await expect(HooksBridge.approvalAnswer("1-1", [0])).resolves.toBe(false);
  });
});
