import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { setLanguage } from "../core/i18n";
import { DEFAULT_SETTINGS } from "../core/state";
import "../core/error-text";
import { BridgeMcpc, type McpcDirectoryEntry } from "../core/bridge-mcpc";
import {
  cleanText, commandLine, customSpec, directorySpec, isServerUrl, needsForm, urlHost, type CustomForm,
} from "./mcp-servers-add";
import { mcpServersSection } from "./mcp-servers";

const FORM: CustomForm = { name: "Mine", kind: "http", url: "https://mcp.example.com/mcp", command: "", args: "", env: "", auth: "none", header: "" };

const FILESYSTEM: McpcDirectoryEntry = {
  id: "filesystem", name: "Filesystem", category: "files", desc: { fa: "پوشه", en: "Folder" }, docsUrl: null,
  transport: {
    type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem"], env: [],
    argFields: [{ index: 2, label: { fa: "پوشه", en: "Folder" }, placeholder: "C:\\", kind: "path" }],
  },
  auth: "none", authHelp: null, needs: "Node.js",
};

beforeAll(() => setLanguage("en"));

describe("mcp servers helpers", () => {
  it("cleans server text: no control or bidi characters, one line, capped", () => {
    expect(cleanText("a\u202Eb\u0007c\n\nd", 100)).toBe("ab c d");
    expect(cleanText("x".repeat(400), 300)).toHaveLength(300);
    expect(cleanText("x".repeat(400), 300).endsWith("…")).toBe(true);
    expect(cleanText(null, 10)).toBe("");
  });

  it("shows a command exactly, quoting only what needs it", () => {
    expect(commandLine("npx", ["-y", "@pkg/server", "C:\\My Docs", ""])).toBe('npx -y @pkg/server "C:\\My Docs" ""');
    expect(commandLine("tool", ['say "hi"'])).toBe('tool "say \\"hi\\""');
  });

  it("accepts https, and http only on this PC", () => {
    expect(isServerUrl("https://mcp.notion.com/mcp")).toBe(true);
    expect(isServerUrl("http://localhost:3000/mcp")).toBe(true);
    expect(isServerUrl("http://127.0.0.1:8080")).toBe(true);
    expect(isServerUrl("http://example.com/mcp")).toBe(false);
    expect(isServerUrl("https://user:pw@example.com")).toBe(false);
    expect(isServerUrl("ftp://example.com")).toBe(false);
    expect(isServerUrl("not a url")).toBe(false);
    expect(urlHost("https://api.githubcopilot.com/mcp/")).toBe("api.githubcopilot.com");
  });

  it("builds a custom HTTP spec and reports what is wrong", () => {
    expect(customSpec(FORM).spec).toEqual({
      name: "Mine", source: "custom", transport: { type: "http", url: "https://mcp.example.com/mcp" }, auth: { type: "none" },
    });
    expect(customSpec({ ...FORM, auth: "header", header: "X-Api-Key" }).spec?.auth).toEqual({ type: "header", name: "X-Api-Key" });
    const bad = customSpec({ ...FORM, name: " ", url: "http://example.com", auth: "header", header: "bad header" });
    expect(bad.spec).toBeNull();
    expect(Object.keys(bad.errors).sort()).toEqual(["header", "name", "url"]);
  });

  it("builds a custom stdio spec: one argument per line, env names checked", () => {
    const ok = customSpec({ ...FORM, kind: "stdio", command: " npx ", args: "-y\n\n  @scope/server  \r\nC:\\My Docs", env: "API_KEY\nAPI_KEY\nOTHER" });
    expect(ok.spec?.transport).toEqual({ type: "stdio", command: "npx", args: ["-y", "@scope/server", "C:\\My Docs"], env: ["API_KEY", "OTHER"] });
    expect(ok.spec?.auth).toEqual({ type: "none" });
    // No value is stored for its variables yet, so it is added switched off.
    expect(ok.spec?.enabled).toBe(false);
    expect(customSpec({ ...FORM, kind: "stdio", command: "npx", args: "", env: "" }).spec?.enabled).toBe(true);
    const bad = customSpec({ ...FORM, kind: "stdio", command: "", args: Array.from({ length: 41 }, (_, i) => `a${i}`).join("\n"), env: "1BAD" });
    expect(Object.keys(bad.errors).sort()).toEqual(["args", "command", "env"]);
  });

  it("fills a directory entry's argument fields", () => {
    const spec = directorySpec(FILESYSTEM, { 2: "  D:\\Projects  " });
    expect(spec.source).toBe("directory:filesystem");
    expect(spec.transport).toEqual({ type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "D:\\Projects"], env: [] });
    expect(needsForm(FILESYSTEM)).toBe(true);
    // A placeholder inside args is replaced in place; appended fields keep their order.
    const two: McpcDirectoryEntry = { ...FILESYSTEM, transport: { type: "stdio", command: "x", args: ["a", "<p>"], env: [], argFields: [
      { index: 3, label: { fa: "", en: "" }, placeholder: "", kind: "text" },
      { index: 1, label: { fa: "", en: "" }, placeholder: "", kind: "text" },
      { index: 2, label: { fa: "", en: "" }, placeholder: "", kind: "text" },
    ] } };
    expect(directorySpec(two, { 1: "P", 2: "B", 3: "C" }).transport).toEqual({ type: "stdio", command: "x", args: ["a", "P", "B", "C"], env: [] });
    const deepwiki: McpcDirectoryEntry = { ...FILESYSTEM, transport: { type: "http", url: "https://mcp.deepwiki.com/mcp" } };
    expect(needsForm(deepwiki)).toBe(false);
    expect(needsForm({ ...deepwiki, auth: { header: "X-Key" } })).toBe(true);
    expect(directorySpec({ ...deepwiki, auth: { header: "X-Key" } }, {}).auth).toEqual({ type: "header", name: "X-Key" });
  });
});

describe("mcp servers section (browser stand-in)", () => {
  afterEach(() => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  });

  it("lists the servers with their status, and approval runs the command only on a click", async () => {
    document.body.innerHTML = "";
    document.body.append(mcpServersSection({ settings: () => ({ ...DEFAULT_SETTINGS }), save: async () => {} }));
    await vi.waitFor(() => expect(document.querySelectorAll(".mcpc-item").length).toBe(4));
    const names = [...document.querySelectorAll(".mcpc-name")].map((e) => e.textContent);
    expect(names).toEqual(["GitHub", "Filesystem", "Notion", "Local tools"]);

    const fs = document.getElementById("mcpc-item-filesystem-a1b2")!;
    expect(fs.textContent).toContain("Approval needed");
    // Opened by itself, with the exact command in an LTR box.
    expect(fs.querySelector<HTMLElement>(".mcpc-panel")!.hidden).toBe(false);
    const code = fs.querySelector<HTMLElement>(".mcpc-code")!;
    expect(code.getAttribute("dir")).toBe("ltr");
    expect(code.textContent).toBe('npx -y @modelcontextprotocol/server-filesystem "C:\\Users\\me\\My Documents"');
    // Nothing to test or list before approval.
    expect(fs.querySelector<HTMLElement>(".mcpc-test")!.hidden).toBe(true);

    // The command changes behind the window's back: the click approves what was
    // shown, so it is refused and the new command takes its place on screen.
    const changed = ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me"];
    await BridgeMcpc.update("filesystem-a1b2", { transport: { type: "stdio", command: "npx", args: changed, env: [] } });
    fs.querySelector<HTMLButtonElement>(".mcpc-approve button")!.click();
    await vi.waitFor(() => expect(fs.querySelector(".mcpc-approve [role=alert]")!.textContent).toContain("changed after it was shown"));
    await vi.waitFor(() => expect(code.textContent).toBe("npx -y @modelcontextprotocol/server-filesystem C:\\Users\\me"));
    expect(fs.textContent).toContain("Approval needed");

    fs.querySelector<HTMLButtonElement>(".mcpc-approve button")!.click();
    await vi.waitFor(() => expect(fs.textContent).toContain("Ready"), { timeout: 4000 });
    await vi.waitFor(() => expect(fs.querySelectorAll(".mcpc-tool").length).toBe(3), { timeout: 4000 });
    expect(fs.querySelector<HTMLElement>(".mcpc-panel")!.hidden).toBe(false);
  });

  it("sets a tool's mode from its segmented control", async () => {
    const gh = document.getElementById("mcpc-item-github-3f9a")!;
    gh.querySelector<HTMLButtonElement>("button.mcpc-id")!.click();
    await vi.waitFor(() => expect(gh.querySelectorAll(".mcpc-tool").length).toBe(5), { timeout: 4000 });
    const del = [...gh.querySelectorAll(".mcpc-tool")].find((r) => r.textContent?.includes("delete_branch"))!;
    expect(del.textContent).toContain("Can change or delete");
    const radios = [...del.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
    radios[2].click();
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "false", "true"]);
  });

  it("won't switch on a local server while a required value is missing", async () => {
    const local = document.getElementById("mcpc-item-local-tools-9d3e")!;
    const sw = local.querySelector<HTMLButtonElement>('[role="switch"]')!;
    sw.click();
    await vi.waitFor(() => expect(local.querySelector(".mcpc-row-err")!.textContent).toContain("API_KEY"));
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(local.querySelector<HTMLElement>(".mcpc-panel")!.hidden).toBe(false);
    expect(local.querySelector(".key-field")!.textContent).toContain("Required");
    const key = local.querySelector<HTMLInputElement>(".key-field input")!;
    expect(key.getAttribute("aria-required")).toBe("true");
    expect(key.getAttribute("aria-invalid")).toBe("true");

    // Storing the value finishes what the click asked for.
    key.value = "secret-value";
    key.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await vi.waitFor(() => expect(sw.getAttribute("aria-checked")).toBe("true"), { timeout: 4000 });
    expect(local.querySelector(".mcpc-row-err")!.textContent).toBe("");
    expect(key.value).toBe("");
    expect(key.hasAttribute("aria-invalid")).toBe(false);
  });

  it("never shows a stored token back", () => {
    const gh = document.getElementById("mcpc-item-github-3f9a")!;
    const input = gh.querySelector<HTMLInputElement>(".key-field input")!;
    expect(input.type).toBe("password");
    expect(input.value).toBe("");
    expect(gh.querySelector(".key-field")!.textContent).toContain("Saved");
  });

  it("asks for a directory server's required values before adding it", async () => {
    document.querySelector<HTMLButtonElement>("#mcpc-add")!.click();
    await vi.waitFor(() => expect(document.querySelector(".mcpc-dlg")).not.toBeNull());
    const brave = [...document.querySelectorAll(".mk-card")].find((c) => c.textContent?.includes("Brave Search"))!;
    brave.querySelector<HTMLButtonElement>("button.sm")!.click();
    const key = document.getElementById("mcpc-env-BRAVE_API_KEY") as HTMLInputElement;
    expect(key.type).toBe("password");
    document.querySelector<HTMLFormElement>(".mcpc-form")!.requestSubmit();
    expect(key.getAttribute("aria-invalid")).toBe("true");
    expect(document.querySelector(".mcpc-dlg")).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".mcpc-dlg")).toBeNull();
  });

  it("adds a custom server from the dialog", async () => {
    document.querySelector<HTMLButtonElement>("#mcpc-add")!.click();
    await vi.waitFor(() => expect(document.querySelector(".mcpc-dlg")).not.toBeNull());
    document.querySelector<HTMLButtonElement>(".mcpc-custom-btn")!.click();
    const set = (id: string, value: string) => {
      const el = document.getElementById(id) as HTMLInputElement;
      el.value = value;
      el.dispatchEvent(new Event("input"));
    };
    // Refused while the URL is plain http to another host.
    set("mcpc-f-name", "Docs");
    set("mcpc-f-url", "http://docs.example.com/mcp");
    document.querySelector<HTMLFormElement>(".mcpc-form")!.requestSubmit();
    expect(document.getElementById("mcpc-f-url")!.getAttribute("aria-invalid")).toBe("true");
    set("mcpc-f-url", "https://docs.example.com/mcp");
    document.querySelector<HTMLFormElement>(".mcpc-form")!.requestSubmit();
    await vi.waitFor(() => expect(document.querySelector(".mcpc-dlg")).toBeNull(), { timeout: 4000 });
    await vi.waitFor(() => expect(document.querySelectorAll(".mcpc-item").length).toBe(5), { timeout: 4000 });
    expect([...document.querySelectorAll(".mcpc-name")].map((e) => e.textContent)).toContain("Docs");
  });
});
