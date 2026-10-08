import { afterEach, describe, expect, it } from "vitest";
import { errorCode, localizeError } from "./error-text";
import { setLanguage } from "./i18n";

const FSI = "⁨";
const PDI = "⁩";
const iso = (s: string) => `${FSI}${s}${PDI}`;

afterEach(() => setLanguage("fa"));

describe("localizeError", () => {
  it("translates a bare code in both languages", () => {
    setLanguage("en");
    expect(localizeError("E_FILE_IS_FOLDER")).toBe("Folders can't be dropped yet.");
    setLanguage("fa");
    expect(localizeError("E_FILE_IS_FOLDER")).toBe("فعلاً نمی‌شود پوشه رها کرد.");
  });

  it("fills arguments, isolating text and formatting numbers", () => {
    setLanguage("en");
    expect(localizeError(String.raw`E_FILE_UNREADABLE|C:\a.pdf|Access is denied.`)).toBe(
      `Can't read ${iso(String.raw`C:\a.pdf`)}: ${iso("Access is denied.")}`,
    );
    expect(localizeError("E_INT_HTTP|503")).toBe("API error 503");
    setLanguage("fa");
    expect(localizeError("E_INT_HTTP|503")).toBe("خطای API (۵۰۳)");
  });

  it("gives the last argument the rest of the line and keeps later lines", () => {
    setLanguage("en");
    expect(localizeError("I_N8N_ITEMS|2|Set | Merge\nname: Ada\nid: 7")).toBe(
      `→ ${iso("Set | Merge")} · 2 item(s)\nname: Ada\nid: 7`,
    );
  });

  it("accepts what a rejected invoke gives", () => {
    setLanguage("en");
    expect(localizeError(new Error("E_FILE_NOT_DROPPED"))).toBe("Drop the file on the island again.");
  });

  it("shows unknown input as it is", () => {
    expect(localizeError("Something odd happened")).toBe("Something odd happened");
    expect(localizeError("E_NOT_A_REAL_CODE|x")).toBe("E_NOT_A_REAL_CODE|x");
    expect(localizeError("")).toBe("");
    expect(localizeError("  {\n+   \"hooks\": {}")).toBe("  {\n+   \"hooks\": {}");
  });

  it("missing arguments do not throw", () => {
    setLanguage("en");
    expect(localizeError("E_CFG_CHANGED")).toContain("changed since the preview");
  });
});

describe("errorCode", () => {
  it("finds known codes only", () => {
    expect(errorCode("E_SHORTCUT_TAKEN|HotKey already registered")).toBe("E_SHORTCUT_TAKEN");
    expect(errorCode("I_NO_CHANGE")).toBe("I_NO_CHANGE");
    expect(errorCode("Error: nope")).toBeNull();
  });
});

describe("integration service codes", () => {
  const codes = [
    "E_INT_NO_KEY", "E_INT_PAUSED", "E_INT_BAD_URL", "E_INT_BAD_ID", "E_INT_API|Rate limited",
    "E_INT_CLOUDFLARE_ACCESS", "E_INT_SENTRY_NO_ORG", "E_INT_UNKNOWN|integration_x",
    "E_INT_BAD_FIELD|Token", "E_INT_NOT_FOUND", "E_INT_RATE_LIMITED", "E_INT_SERVER|503", "E_INT_TOO_LARGE",
    "E_INT_BAD_RESPONSE", "E_INT_HOST_BLOCKED",
  ];

  it("are translated in both languages", () => {
    for (const lang of ["en", "fa"] as const) {
      setLanguage(lang);
      for (const code of codes) {
        const text = localizeError(code);
        expect(text, `${lang} ${code}`).not.toContain("E_INT_");
        expect(text.length).toBeGreaterThan(0);
      }
    }
  });

  it("keep the service's own message as an isolated argument", () => {
    setLanguage("en");
    expect(localizeError("E_INT_API|Rate limited")).toBe(`The service said: ${iso("Rate limited")}`);
  });

  it("name the catalog field and format the server status", () => {
    setLanguage("en");
    expect(localizeError("E_INT_BAD_FIELD|Site URL")).toBe(`Check “${iso("Site URL")}” — it doesn't look right`);
    setLanguage("fa");
    expect(localizeError("E_INT_SERVER|503")).toBe("سرویس به مشکل خورده (۵۰۳)");
  });
});
