export const CODING_PROVIDERS = {
  claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", cursor: "Cursor",
  windsurf: "Windsurf", copilot: "GitHub Copilot", vscode: "VS Code", cline: "Cline",
  kiro: "Kiro", opencode: "OpenCode", hermes: "Hermes", antigravity: "Antigravity",
} as const;
export type CodingProvider = keyof typeof CODING_PROVIDERS;
export function isCodingProvider(value: unknown): value is CodingProvider {
  return typeof value === "string" && Object.hasOwn(CODING_PROVIDERS, value);
}
export const codingTaskId = (provider: CodingProvider) => `integration_${provider}`;
