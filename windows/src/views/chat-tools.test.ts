import { describe, expect, it } from "vitest";
import { localCallDetails, toolStepRow } from "./chat";
import { setLanguage } from "../core/i18n";
import type { ChatToolStep } from "../core/bridge";

const step = (over: Partial<ChatToolStep> = {}): ChatToolStep => ({
  id: "3-1",
  server: "GitHub",
  tool: "create_issue",
  state: "done",
  arguments: '{\n  "title": "x"\n}',
  result: "Issue #12 created",
  error: null,
  ...over,
});

describe("tool-step rows", () => {
  it("name the server and tool, say the state, and open on a click", () => {
    setLanguage("en");
    const row = toolStepRow(step());
    expect(row.classList.contains("done")).toBe(true);
    expect(row.querySelector(".tool-step-name")?.textContent).toBe("GitHub · create_issue");
    expect(row.querySelector(".tool-step-state")?.textContent).toBe("Done");
    const head = row.querySelector<HTMLButtonElement>(".tool-step-head")!;
    const body = row.querySelector<HTMLElement>(".tool-step-body")!;
    expect(body.hidden).toBe(true);
    expect(head.getAttribute("aria-expanded")).toBe("false");
    head.click();
    expect(body.hidden).toBe(false);
    expect(head.getAttribute("aria-expanded")).toBe("true");
    expect([...body.querySelectorAll("pre")].map((p) => p.textContent)).toEqual(['{\n  "title": "x"\n}', "Issue #12 created"]);
    // Opened rows stay open when the turn repaints them.
    expect(toolStepRow(step()).querySelector<HTMLElement>(".tool-step-body")!.hidden).toBe(false);
    head.click();
  });

  it("show server text as text, never as markup, and drop bidi overrides", () => {
    const row = toolStepRow(step({ id: "x", server: "Evil‮", result: '<img src=x onerror="alert(1)">' }));
    expect(row.querySelector("img")).toBeNull();
    expect(row.querySelector(".tool-step-name")?.textContent).toBe("Evil · create_issue");
    expect(row.querySelectorAll("pre")[1].textContent).toBe('<img src=x onerror="alert(1)">');
  });

  it("show a declined step without a result, and an error in the UI language", () => {
    setLanguage("fa");
    const declined = toolStepRow(step({ id: "d", state: "declined", result: null, arguments: "{}" }));
    expect(declined.querySelector(".tool-step-state")?.textContent).toBe("رد شد");
    expect(declined.querySelectorAll("pre")).toHaveLength(0);
    expect(declined.querySelector(".tool-step-none")).not.toBeNull();
    const failed = toolStepRow(step({ id: "e", state: "error", result: null, error: "E_NOT_A_KNOWN_CODE" }));
    expect(failed.querySelector(".tool-step-err")?.textContent).toBe("E_NOT_A_KNOWN_CODE");
    expect(failed.querySelector(".tool-step-ico")).not.toBeNull();
    setLanguage("en");
  });

  it("approvals of a local tool show the exact input in full, left to right, as text", () => {
    setLanguage("fa");
    const json = '{\n  "path": "C:\\\\x",\n  "api_key": "sk-123",\n  "html": "<img src=x onerror=alert(1)>"\n}';
    const el = localCallDetails({ server: "فایل‌ها", tool: "write_file", arguments: json });
    const box = el.querySelector<HTMLElement>(".appr-sum")!;
    expect(box.getAttribute("dir")).toBe("ltr");
    expect(box.textContent).toBe(json);
    expect(box.textContent).toContain("sk-123");
    expect(el.querySelector("img")).toBeNull();
    expect([...el.querySelectorAll(".appr-call-v")].map((v) => v.textContent)).toEqual(["فایل‌ها", "write_file"]);
    expect(el.querySelector(".appr-call-k")?.textContent).toBe("سرور");
    setLanguage("en");
  });
});
