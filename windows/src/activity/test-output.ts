import type { TestResult } from "./types";

export function isTestCommand(command: string): boolean {
  if (/^\s*(?:echo|printf|cat|rg|grep)\b|<<|[\r\n]/.test(command)) return false;
  // Quoted arguments are data, not evidence that a runner was invoked.
  command = command.replace(/"[^"\r\n]*"|'[^'\r\n]*'/g, "");
  return /(?:^|[\s;&|])(?:pytest|vitest|jest|mocha)(?:\s|$)|\b(?:cargo|go|dotnet|bun|deno)\s+test\b|\b(?:npm|pnpm|yarn)\s+(?:run\s+)?test(?:[:\s]|$)|\b(?:python(?:3)?\s+-m\s+(?:pytest|unittest)|node\s+--test)\b/.test(command);
}

/** Exit code zero alone does not establish that any tests ran. */
export function parseTestOutput(output: string, exitCode?: number): TestResult {
  let passed = 0;
  let failed = 0;
  let skipped = 0;
  const text = output.replace(/\x1b\[[0-9;]*m/g, "");
  const rust = /test result:\s*(ok|FAILED)\.\s*(\d+) passed;\s*(\d+) failed;\s*(\d+) ignored/gi;
  let match: RegExpExecArray | null;
  let structured = false;
  while ((match = rust.exec(text))) {
    structured = true;
    passed += Number(match[2]); failed += Number(match[3]); skipped += Number(match[4]);
  }
  if (!structured) {
    // Prefer test rows over suite/file rows, so a passing suite cannot mask failures.
    const rows = text.split(/\r?\n/).filter(line => /^\s*(?:Tests\s|# (?:pass|fail|skipped)\s)|\b\d+ (?:passed|failed|skipped|pending)\b/.test(line) && !/^\s*(?:Test Files|Test Suites|Suites)\b/.test(line));
    for (const row of rows) {
      for (const field of row.matchAll(/(\d+)\s+(passed|failed|skipped|pending)\b/gi)) {
        const count = Number(field[1]);
        if (field[2].toLowerCase() === "passed") passed += count;
        else if (field[2].toLowerCase() === "failed") failed += count;
        else skipped += count;
      }
      const node = /^\s*# (pass|fail|skipped)\s+(\d+)/.exec(row);
      if (node) { if (node[1] === "pass") passed += Number(node[2]); else if (node[1] === "fail") failed += Number(node[2]); else skipped += Number(node[2]); }
    }
  }
  const incomplete = /\[truncated\]|output truncated|running\.\.\.|watching for file changes/i.test(text);
  if (failed > 0 || (exitCode !== undefined && exitCode !== 0)) return { verdict: "failed", passed, failed, skipped, reason: failed ? "Reported test failures" : "Command exited unsuccessfully" };
  if (incomplete || exitCode === undefined) return { verdict: "unknown", passed, failed, skipped, reason: incomplete ? "Output is incomplete" : "Exit status is unknown" };
  if (passed > 0) return { verdict: "passed", passed, failed, skipped, reason: "Successful exit with reported passing tests" };
  if (skipped > 0) return { verdict: "skipped", passed, failed, skipped, reason: "Only skipped tests were reported" };
  return { verdict: "unknown", passed, failed, skipped, reason: "No executed test count was found" };
}
