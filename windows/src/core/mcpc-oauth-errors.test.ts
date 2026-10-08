import { afterEach, describe, expect, it } from "vitest";
import { errorCode, localizeError } from "./error-text";
import { setLanguage } from "./i18n";

// The codes of src-tauri/src/mcpc/oauth.rs (errors.rs, MCPC_OAUTH_*).
const CODES = [
  "E_MCPC_OAUTH_DISCOVERY|no token_endpoint",
  "E_MCPC_OAUTH_INSECURE|auth.example.com",
  "E_MCPC_OAUTH_NO_DCR",
  "E_MCPC_OAUTH_REGISTER|400 invalid_client_metadata",
  "E_MCPC_OAUTH_LISTEN|os error 10013",
  "E_MCPC_OAUTH_DENIED|access_denied",
  "E_MCPC_OAUTH_STATE",
  "E_MCPC_OAUTH_TIMEOUT",
  "E_MCPC_OAUTH_CANCELLED",
  "E_MCPC_OAUTH_TOKEN|400 invalid_grant",
  "E_MCPC_OAUTH_KEYRING|Access is denied.",
];

afterEach(() => setLanguage("fa"));

describe("MCP sign-in errors", () => {
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

  it("keeps the argument, isolated", () => {
    setLanguage("fa");
    expect(localizeError("E_MCPC_OAUTH_INSECURE|auth.example.com")).toContain("⁨auth.example.com⁩");
    setLanguage("en");
    expect(localizeError("E_MCPC_OAUTH_DENIED|access_denied")).toContain("access_denied");
  });
});
