// English strings for the island's waiting cards (questions, folding), the
// live diff and the coding agents' wording. Registered by r2-messages.ts.

export const r2En: Record<string, string> = {
  // Live diff: the ticker's diff rows and the diff card.
  "live.showDiff": "Show the changes",
  "live.openFile": "Open in VS Code",
  "live.tooLarge": "Too large to show",
  "live.noChanges": "No changes",
  // A permission card or a question folded away without an answer.
  "card.fold": "Later — keep it waiting",
  // Next to a coding agent's name (Codex, Gemini CLI…): what kind of pill it is.
  "card.agent": "Agent",
  "card.agentFinished": "finished",
  // Claude Code's questions (AskUserQuestion), answered from the island.
  "ask.asking": "is asking",
  "ask.askingOf": "is asking ({index} of {total})",
  "ask.askingQuestion": "is asking a question",
  "ask.done": "Done",
  "ask.inTerminal": "Answer in terminal",
  "ask.answerItInTerminal": "Answer it in your terminal.",
};
