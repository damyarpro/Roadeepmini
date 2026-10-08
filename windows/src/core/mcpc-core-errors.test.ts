import { afterEach, describe, expect, it } from "vitest";
import { errorCode, localizeError } from "./error-text";
import { setLanguage } from "./i18n";

// The codes of the MCP client core (src-tauri/src/errors.rs, MCPC_* without OAUTH).
const CODES = [
  "E_MCPC_STORE|Access is denied. (os error 5)",
  "E_MCPC_UNKNOWN_SERVER|gh-3f9a",
  "E_MCPC_INVALID|url",
  "E_MCPC_LIMIT|50",
  "E_MCPC_NEEDS_AUTH",
  "E_MCPC_NEEDS_APPROVAL",
  "E_MCPC_DISABLED",
  "E_MCPC_COMMAND_NOT_FOUND|npx",
  "E_MCPC_UNSAFE_ARG|%PATH%",
  "E_MCPC_SPAWN|The system cannot find the file specified. (os error 2)",
  "E_MCPC_EXITED",
  "E_MCPC_TIMEOUT",
  "E_MCPC_HTTP|500",
  "E_MCPC_NETWORK|error sending request: connection refused",
  "E_MCPC_TOO_LARGE",
  "E_MCPC_PROTOCOL|the answer is not JSON",
  "E_MCPC_RPC|-32602|Invalid params | missing path",
  "E_MCPC_UNKNOWN_TOOL|github__nope",
  "E_MCPC_TOOL_OFF|github__delete_repo",
  "E_MCPC_TOOL_ASK|create_issue",
  "E_MCPC_ARGS_TOO_LARGE|20000",
  "E_MCPC_COMMAND_CHANGED",
];

afterEach(() => setLanguage("fa"));

describe("MCP client errors", () => {
  it("are known codes with text in both languages", () => {
    for (const raw of CODES) {
      expect(errorCode(raw)).toBe(raw.split("|")[0]);
      for (const lang of ["en", "fa"] as const) {
        setLanguage(lang);
        const text = localizeError(raw);
        expect(text).not.toContain("E_MCPC");
        expect(text).not.toContain("err.mcpc");
        expect(text).not.toMatch(/\{\w+\}/);
      }
    }
  });

  it("keeps the server's stderr lines after an exit", () => {
    setLanguage("en");
    expect(localizeError("E_MCPC_EXITED\nError: Cannot find module 'x'")).toBe("The server stopped.\nError: Cannot find module 'x'");
  });

  it("gives the last argument the rest of the line", () => {
    setLanguage("en");
    expect(localizeError("E_MCPC_RPC|-32602|Invalid params | missing path")).toContain("Invalid params | missing path");
    setLanguage("fa");
    expect(localizeError("E_MCPC_HTTP|500")).toContain("۵۰۰");
  });
});
