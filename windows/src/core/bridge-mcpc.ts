// Tauri commands for the MCP servers the user connects to the app (the app is
// the MCP *client* here; src-tauri/src/mcpc/). Kept apart from bridge-mcp.ts,
// which is about Roadeep served AS an MCP server.
//
// Outside the app (`npm run dev` in a browser) an in-memory stand-in answers,
// so the settings section can be laid out and reviewed without Rust.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI, onEvent } from "./bridge";

export type McpcStatus = "off" | "idle" | "connecting" | "ready" | "needs_auth" | "needs_approval" | "error";
export type McpcToolMode = "auto" | "ask" | "off";
export type McpcAuthType = "none" | "bearer" | "header" | "oauth";

export interface McpcServerView {
  id: string;
  name: string;
  /** "directory:<entry id>" or "custom". */
  source: string;
  enabled: boolean;
  transport:
    | { type: "http"; url: string }
    | { type: "stdio"; command: string; args: string[]; env: { name: string; present: boolean }[] };
  /** `present`: a token or OAuth tokens are stored. `header`: the header name (type "header"). */
  auth: { type: McpcAuthType; header?: string; present: boolean };
  status: McpcStatus;
  /** A coded error (core/error-text.ts), or null. */
  error: string | null;
  toolCount: number;
  serverInfo: { name: string; version: string } | null;
  /**
   * stdio: the hash of the command, arguments and variable names shown.
   * Approval sends back the one the user saw; null for HTTP.
   */
  commandHash: string | null;
}

export interface McpcToolView {
  name: string;
  title: string | null;
  /** Untrusted text from the server, ≤ 300 chars. */
  description: string;
  readOnly: boolean;
  destructive: boolean;
  mode: McpcToolMode;
}

/** What mcpc_add takes: the config-file shape (env = names only, values go to the keyring). */
export type McpcTransportSpec =
  | { type: "http"; url: string }
  | { type: "stdio"; command: string; args: string[]; env: string[] };

export type McpcAuthSpec =
  | { type: "none" }
  | { type: "bearer" }
  | { type: "header"; name: string }
  | { type: "oauth" };

export interface McpcAddSpec {
  name: string;
  source: string;
  transport: McpcTransportSpec;
  auth: McpcAuthSpec;
  enabled?: boolean;
}

export interface McpcPatch {
  name?: string;
  enabled?: boolean;
  transport?: McpcTransportSpec;
  auth?: McpcAuthSpec;
}

/** "token" (bearer or header value) or "env:<NAME>". */
export type McpcSecretSlot = "token" | `env:${string}`;

export interface McpcConnectResult {
  serverInfo: { name: string; version: string } | null;
  tools: McpcToolView[];
}

export type McpcCategory = "dev" | "work" | "data" | "web" | "files" | "design" | "commerce" | "docs" | "other";
export const MCPC_CATEGORIES: McpcCategory[] = ["dev", "work", "data", "web", "files", "design", "commerce", "docs", "other"];

export interface McpcLocalized { fa: string; en: string }

export interface McpcDirectoryEntry {
  id: string;
  name: string;
  category: McpcCategory;
  desc: McpcLocalized;
  docsUrl: string | null;
  transport:
    | { type: "http"; url: string }
    | {
        type: "stdio"; command: string; args: string[];
        env: { name: string; label: McpcLocalized; secret: boolean; placeholder: string }[];
        argFields?: { index: number; label: McpcLocalized; placeholder: string; kind: "path" | "text" }[];
      };
  auth: "none" | "bearer" | "oauth" | { header: string };
  /** Where to get a token. */
  authHelp: McpcLocalized | null;
  /** e.g. "Node.js" or "uv" for a local server. */
  needs: string | null;
}

/** Sent to both windows on every status change. */
export interface McpcStatusEvent {
  id: string;
  status: McpcStatus;
  error: string | null;
}

export const MCPC_STATUS_EVENT = "mcpc-status";

/** Rejects with the coded error string Rust sent, so the UI can localize it. */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) return mock(cmd, args ?? {}) as Promise<T>;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    // The code only: messages can quote a server's own text.
    void Bridge.log(`settings: ${cmd} failed: ${String(err).split(/[|\n]/, 1)[0]}`);
    throw err;
  }
}

export const BridgeMcpc = {
  list: () => call<McpcServerView[]>("mcpc_list"),
  /** Resolves to the new server's id. */
  add: (spec: McpcAddSpec) => call<string>("mcpc_add", { spec }),
  update: (id: string, patch: McpcPatch) => call<void>("mcpc_update", { id, patch }),
  /** Stops the server and clears every secret it had. */
  remove: (id: string) => call<void>("mcpc_remove", { id }),
  setSecret: (id: string, slot: McpcSecretSlot, value: string) => call<void>("mcpc_set_secret", { id, slot, value }),
  clearSecret: (id: string, slot: McpcSecretSlot) => call<void>("mcpc_clear_secret", { id, slot }),
  /**
   * Only from the explicit click under the exact command shown. `hash` is the
   * commandHash of the view that was on screen: a command changed since then
   * is refused (E_MCPC_COMMAND_CHANGED), never approved unseen.
   */
  approveCommand: (id: string, hash: string) => call<void>("mcpc_approve_command", { id, hash }),
  /** (Re)connects and lists the tools: the "Test connection" button. */
  connect: (id: string) => call<McpcConnectResult>("mcpc_connect", { id }),
  tools: (id: string) => call<McpcToolView[]>("mcpc_tools", { id }),
  setToolMode: (id: string, tool: string, mode: McpcToolMode) => call<void>("mcpc_set_tool_mode", { id, tool, mode }),
  /** Opens the browser; resolves once signed in (or rejects with a coded error / timeout). */
  oauthStart: (id: string) => call<void>("mcpc_oauth_start", { id }),
  oauthSignout: (id: string) => call<void>("mcpc_oauth_signout", { id }),
  directory: () => call<McpcDirectoryEntry[]>("mcpc_directory"),
};

/** Live status changes. Resolves to the unlisten function. */
export function onMcpcStatus(handler: (event: McpcStatusEvent) => void): Promise<() => void> {
  if (!IS_TAURI) {
    mockListeners.add(handler);
    return Promise.resolve(() => mockListeners.delete(handler));
  }
  return onEvent<McpcStatusEvent>(MCPC_STATUS_EVENT, handler);
}

// ── Browser stand-in ──────────────────────────────────────────────────────────
// Just enough behaviour to review the section in a browser: nothing here runs,
// connects or stores anything.

const mockListeners = new Set<(event: McpcStatusEvent) => void>();

interface MockServer {
  view: McpcServerView;
  tools: McpcToolView[];
  /** Has a token / env value / OAuth tokens per slot. */
  secrets: Set<string>;
  approved: boolean;
}

const mockTool = (name: string, description: string, readOnly: boolean, destructive = false): McpcToolView =>
  ({ name, title: null, description, readOnly, destructive, mode: readOnly ? "auto" : "ask" });

const MOCK_TOOLS: Record<string, McpcToolView[]> = {
  github: [
    mockTool("search_code", "Search code across GitHub repositories.", true),
    mockTool("get_issue", "Get the details of an issue in a repository.", true),
    mockTool("create_issue", "Open a new issue in a repository.", false),
    mockTool("create_pull_request", "Open a pull request from a branch.", false),
    mockTool("delete_branch", "Delete a branch from a repository.", false, true),
  ],
  filesystem: [
    mockTool("read_file", "Read the complete contents of a file.", true),
    mockTool("list_directory", "List the files and folders in a directory.", true),
    mockTool("write_file", "Create a new file or overwrite an existing one.", false, true),
  ],
  notion: [mockTool("search", "Search pages and databases in the workspace.", true)],
};

let mockServers: MockServer[] | null = null;
let mockSeq = 0;

function mockState(): MockServer[] {
  if (mockServers) return mockServers;
  const server = (view: Omit<McpcServerView, "toolCount" | "serverInfo" | "commandHash">, tools: McpcToolView[], secrets: string[], approved = true) =>
    ({ view: { ...view, commandHash: mockHash(view.transport), toolCount: view.status === "ready" ? tools.length : 0, serverInfo: view.status === "ready" ? { name: view.name, version: "1.0.0" } : null }, tools, secrets: new Set(secrets), approved });
  mockServers = [
    server({
      id: "github-3f9a", name: "GitHub", source: "directory:github", enabled: true,
      transport: { type: "http", url: "https://api.githubcopilot.com/mcp/" },
      auth: { type: "bearer", present: true }, status: "ready", error: null,
    }, MOCK_TOOLS.github, ["token"]),
    server({
      id: "filesystem-a1b2", name: "Filesystem", source: "directory:filesystem", enabled: true,
      transport: {
        type: "stdio", command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me\\My Documents"],
        env: [],
      },
      auth: { type: "none", present: false }, status: "needs_approval", error: null,
    }, MOCK_TOOLS.filesystem, [], false),
    server({
      id: "notion-77c0", name: "Notion", source: "directory:notion", enabled: true,
      transport: { type: "http", url: "https://mcp.notion.com/mcp" },
      auth: { type: "oauth", present: false }, status: "needs_auth", error: null,
    }, MOCK_TOOLS.notion, []),
    server({
      id: "local-tools-9d3e", name: "Local tools", source: "custom", enabled: false,
      transport: { type: "stdio", command: "uvx", args: ["my-mcp-server"], env: [{ name: "API_KEY", present: false }] },
      auth: { type: "none", present: false }, status: "off", error: null,
    }, [], []),
  ];
  return mockServers;
}

const MOCK_DIRECTORY: McpcDirectoryEntry[] = [
  {
    id: "github", name: "GitHub", category: "dev",
    desc: { fa: "مخزن‌ها، ایشوها و درخواست‌های ادغام", en: "Repositories, issues and pull requests" },
    docsUrl: "https://github.com/github/github-mcp-server",
    transport: { type: "http", url: "https://api.githubcopilot.com/mcp/" },
    auth: "bearer",
    authHelp: { fa: "یک توکن دسترسی شخصی در تنظیمات توسعه‌دهندهٔ گیت‌هاب بساز.", en: "Create a personal access token in GitHub's developer settings." },
    needs: null,
  },
  {
    id: "notion", name: "Notion", category: "work",
    desc: { fa: "صفحه‌ها و پایگاه‌داده‌های فضای کارت", en: "Pages and databases in your workspace" },
    docsUrl: null, transport: { type: "http", url: "https://mcp.notion.com/mcp" }, auth: "oauth", authHelp: null, needs: null,
  },
  {
    id: "linear", name: "Linear", category: "work",
    desc: { fa: "ایشوها، پروژه‌ها و چرخه‌ها", en: "Issues, projects and cycles" },
    docsUrl: null, transport: { type: "http", url: "https://mcp.linear.app/mcp" }, auth: "oauth", authHelp: null, needs: null,
  },
  {
    id: "deepwiki", name: "DeepWiki", category: "docs",
    desc: { fa: "مستندات مخزن‌های عمومی گیت‌هاب", en: "Documentation of public GitHub repositories" },
    docsUrl: null, transport: { type: "http", url: "https://mcp.deepwiki.com/mcp" }, auth: "none", authHelp: null, needs: null,
  },
  {
    id: "stripe", name: "Stripe", category: "commerce",
    desc: { fa: "مشتری‌ها، پرداخت‌ها و صورت‌حساب‌ها", en: "Customers, payments and invoices" },
    docsUrl: null, transport: { type: "http", url: "https://mcp.stripe.com" }, auth: "bearer",
    authHelp: { fa: "یک کلید محدود در داشبورد استرایپ بساز.", en: "Create a restricted key in the Stripe dashboard." }, needs: null,
  },
  {
    id: "filesystem", name: "Filesystem", category: "files",
    desc: { fa: "خواندن و نوشتن فایل‌های یک پوشه", en: "Read and write the files in one folder" },
    docsUrl: null,
    transport: {
      type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem"], env: [],
      argFields: [{ index: 2, label: { fa: "پوشه", en: "Folder" }, placeholder: "C:\\Users\\me\\Documents", kind: "path" }],
    },
    auth: "none", authHelp: null, needs: "Node.js",
  },
  {
    id: "fetch", name: "Fetch", category: "web",
    desc: { fa: "خواندن صفحه‌های وب به صورت متن", en: "Read web pages as text" },
    docsUrl: null, transport: { type: "stdio", command: "uvx", args: ["mcp-server-fetch"], env: [] }, auth: "none", authHelp: null, needs: "uv",
  },
  {
    id: "brave", name: "Brave Search", category: "web",
    desc: { fa: "جستجوی وب با Brave", en: "Web search with Brave" },
    docsUrl: null,
    transport: {
      type: "stdio", command: "npx", args: ["-y", "@brave/brave-search-mcp-server"],
      env: [{ name: "BRAVE_API_KEY", label: { fa: "کلید API", en: "API key" }, secret: true, placeholder: "BSA…" }],
    },
    auth: "none", authHelp: null, needs: "Node.js",
  },
];

const delay = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function mockEmit(s: MockServer) {
  for (const listener of mockListeners) listener({ id: s.view.id, status: s.view.status, error: s.view.error });
}

function mockSync(s: MockServer) {
  const v = s.view;
  if (v.transport.type === "stdio") {
    for (const env of v.transport.env) env.present = s.secrets.has(`env:${env.name}`);
  }
  v.auth.present = s.secrets.has(v.auth.type === "oauth" ? "oauth" : "token");
  const needsAuth = (v.auth.type === "bearer" || v.auth.type === "header" || v.auth.type === "oauth") && !v.auth.present;
  v.status = !v.enabled ? "off"
    : v.transport.type === "stdio" && !s.approved ? "needs_approval"
    : needsAuth ? "needs_auth"
    : v.status === "ready" ? "ready" : "idle";
  v.toolCount = v.status === "ready" ? s.tools.filter((t) => t.mode !== "off").length : v.toolCount;
}

/** Stands in for the SHA-256 of command + args + variable names (store::command_hash). */
const mockHash = (t: McpcServerView["transport"]): string | null =>
  t.type === "stdio" ? `mock:${JSON.stringify([t.command, t.args, t.env.map((e) => e.name)])}` : null;

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

async function mock(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  await delay(cmd === "mcpc_connect" || cmd === "mcpc_oauth_start" ? 900 : 60);
  const servers = mockState();
  const find = () => {
    const s = servers.find((x) => x.view.id === args.id);
    if (!s) throw new Error("E_MCPC_UNKNOWN_SERVER");
    return s;
  };
  switch (cmd) {
    case "mcpc_list": return clone(servers.map((s) => s.view));
    case "mcpc_directory": return clone(MOCK_DIRECTORY);
    case "mcpc_add": {
      const spec = args.spec as McpcAddSpec;
      const id = `${spec.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "server"}-${(++mockSeq).toString(16).padStart(4, "0")}`;
      const t = spec.transport;
      const view: McpcServerView = {
        id, name: spec.name, source: spec.source, enabled: spec.enabled ?? true,
        transport: t.type === "http" ? { ...t } : { type: "stdio", command: t.command, args: [...t.args], env: t.env.map((name) => ({ name, present: false })) },
        auth: { type: spec.auth.type, header: spec.auth.type === "header" ? spec.auth.name : undefined, present: false },
        status: "idle", error: null, toolCount: 0, serverInfo: null, commandHash: null,
      };
      view.commandHash = mockHash(view.transport);
      const entry = spec.source.startsWith("directory:") ? spec.source.slice("directory:".length) : "";
      const s: MockServer = { view, tools: clone(MOCK_TOOLS[entry] ?? []), secrets: new Set(), approved: false };
      mockSync(s);
      servers.push(s);
      return id;
    }
    case "mcpc_update": {
      const s = find();
      const patch = args.patch as McpcPatch;
      if (patch.enabled !== undefined) s.view.enabled = patch.enabled;
      if (patch.name !== undefined) s.view.name = patch.name;
      const t = patch.transport;
      if (t?.type === "stdio" && s.view.transport.type === "stdio") {
        // Like the store: a different command must be confirmed again.
        s.view.transport = { type: "stdio", command: t.command, args: [...t.args], env: t.env.map((name) => ({ name, present: false })) };
        const hash = mockHash(s.view.transport);
        if (hash !== s.view.commandHash) s.approved = false;
        s.view.commandHash = hash;
      }
      mockSync(s);
      mockEmit(s);
      return null;
    }
    case "mcpc_remove": {
      const s = find();
      servers.splice(servers.indexOf(s), 1);
      return null;
    }
    case "mcpc_set_secret":
    case "mcpc_clear_secret": {
      const s = find();
      if (cmd === "mcpc_set_secret") s.secrets.add(String(args.slot));
      else s.secrets.delete(String(args.slot));
      mockSync(s);
      mockEmit(s);
      return null;
    }
    case "mcpc_approve_command": {
      const s = find();
      if (args.hash !== s.view.commandHash) throw new Error("E_MCPC_COMMAND_CHANGED");
      s.approved = true;
      mockSync(s);
      mockEmit(s);
      return null;
    }
    case "mcpc_connect":
    case "mcpc_tools": {
      const s = find();
      if (s.view.status === "needs_approval") throw new Error("E_MCPC_NEEDS_APPROVAL");
      if (s.view.status === "needs_auth") throw new Error("E_MCPC_NEEDS_AUTH");
      if (s.view.status === "off") throw new Error("E_MCPC_DISABLED");
      s.view.status = "ready";
      s.view.error = null;
      s.view.serverInfo = { name: s.view.name, version: "1.0.0" };
      mockSync(s);
      mockEmit(s);
      return cmd === "mcpc_tools" ? clone(s.tools) : { serverInfo: clone(s.view.serverInfo), tools: clone(s.tools) };
    }
    case "mcpc_set_tool_mode": {
      const s = find();
      const tool = s.tools.find((x) => x.name === args.tool);
      if (tool) tool.mode = args.mode as McpcToolMode;
      mockSync(s);
      return null;
    }
    case "mcpc_oauth_start": {
      const s = find();
      s.secrets.add("oauth");
      mockSync(s);
      mockEmit(s);
      return null;
    }
    case "mcpc_oauth_signout": {
      const s = find();
      s.secrets.delete("oauth");
      if (s.view.status === "ready") s.view.status = "idle";
      mockSync(s);
      mockEmit(s);
      return null;
    }
    default:
      throw new Error(`mock: unknown command ${cmd}`);
  }
}
