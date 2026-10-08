// Runs the actual generated adapter against documented SDK fixtures and an isolated child-process transport.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { EventEmitter } from "node:events";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import ts from "../../node_modules/typescript/lib/typescript.js";

const source = (await readFile(new URL("index.template.ts", import.meta.url), "utf8")).replace('"__ROADEEP_RELAY__"', JSON.stringify("C:/Trusted/roadeep-hook.exe"));
const program = ts.createProgram([fileURLToPath(new URL("index.template.ts", import.meta.url)), fileURLToPath(new URL("documented-sdk.fixture.d.ts", import.meta.url))], {
  noEmit: true, strict: true, skipLibCheck: true, module: ts.ModuleKind.NodeNext, moduleResolution: ts.ModuleResolutionKind.NodeNext, target: ts.ScriptTarget.ES2022,
  typeRoots: [fileURLToPath(new URL("../../node_modules/@types", import.meta.url))], types: ["node"],
});
const typeErrors = ts.getPreEmitDiagnostics(program).filter(d => d.category === ts.DiagnosticCategory.Error);
assert.equal(typeErrors.length, 0, typeErrors.map(d => ts.flattenDiagnosticMessageText(d.messageText, "\n")).join("\n"));
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }, reportDiagnostics: true });
assert.equal(compiled.diagnostics.filter(d => d.category === ts.DiagnosticCategory.Error).length, 0);
const launched = [], timers = new Map(), warnings = [], hooks = new Map();
let nextTimer = 1, subscriberSignal, releaseEvent;
class Child extends EventEmitter {
  constructor() { super(); this.stdin = new EventEmitter(); this.stdin.end = payload => { this.payload = JSON.parse(payload); }; }
  kill() { this.killed = true; }
}
const context = vm.createContext({ AbortController, Date, Map, Set, JSON, console: { warn: value => warnings.push(value) },
  setTimeout: callback => { const id = nextTimer++; timers.set(id, () => { timers.delete(id); callback(); }); return id; }, clearTimeout: id => timers.delete(id),
});
const processModule = new vm.SyntheticModule(["spawn"], function () { this.setExport("spawn", (exe, args, options) => { const child = new Child(); launched.push({ exe, args, options, child }); return child; }); }, { context });
const pluginModule = new vm.SyntheticModule(["Plugin"], function () { this.setExport("Plugin", { define: value => value }); }, { context });
const module = new vm.SourceTextModule(compiled.outputText, { context });
await module.link(specifier => specifier === "node:child_process" ? processModule : pluginModule);
await module.evaluate();
let disposed = 0;
const register = async (name, callback) => { hooks.set(name, callback); return { dispose() { disposed++; } }; };
const cleanup = await module.namespace.default.setup({
  location: { directory: "C:/SyntheticProject" }, session: { hook: register }, tool: { hook: register },
  event: { async *subscribe({ signal }) { subscriberSignal = signal; while (!signal.aborted) { const event = await new Promise(resolve => { releaseEvent = resolve; signal.addEventListener("abort", () => resolve(null), { once: true }); }); if (event) yield event; } } },
});
assert.deepEqual([...hooks.keys()], ["prompt", "execute.before", "execute.after"]);
const input = { sessionID: "synthetic-session", tool: "Bash", callID: "call-1", input: { command: "DO NOT FORWARD", content: "private" }, result: { output: "private" } };
hooks.get("execute.before")(input);
assert.equal(launched.length, 1);
assert.equal(launched[0].exe, "C:/Trusted/roadeep-hook.exe");
assert.equal(JSON.stringify(launched[0].args), JSON.stringify(["--provider", "opencode", "tool.execute.before"]));
assert.equal(launched[0].options.shell, false);
assert.equal(launched[0].options.windowsHide, true);
assert.equal(launched[0].child.payload.tool_use_id, "call-1");
assert.ok(!JSON.stringify(launched[0].child.payload).includes("private"));
assert.ok(!JSON.stringify(launched[0].child.payload).includes("DO NOT FORWARD"));
assert.equal(input.input.content, "private", "adapter must not mutate tool input");
hooks.get("execute.after")({ ...input, status: "error", error: { message: "private" } });
assert.equal(launched[1].args[2], "tool.execute.failed");
hooks.get("prompt")({ sessionID: "synthetic-session", prompt: { text: "private" } });
assert.equal(launched[2].args[2], "session.prompt");
releaseEvent({ type: "permission.asked", properties: { sessionID: "synthetic-session", metadata: { token: "private" } } });
await new Promise(resolve => setImmediate(resolve));
assert.equal(launched[3].args[2], "permission.asked");
assert.equal(launched[3].child.payload.notification_type, "permission_prompt");
assert.equal(launched[3].options.stdio[1], "ignore", "observation cannot consume approval output");
releaseEvent({ type: "session.created", properties: { sessionID: "other-session", info: { id: "other-session", directory: "C:/OtherProject" } } });
await new Promise(resolve => setImmediate(resolve));
assert.equal(launched.length, 4, "server-wide events from another project must be ignored");
for (let i = 0; i < 30; i++) hooks.get("execute.before")({ ...input, callID: `call-${i}` });
assert.equal(launched.length, 16, "in-flight relay processes are bounded");
const first = launched[0].child; first.emit("error", new Error("PRIVATE ERROR"));
hooks.get("execute.before")(input);
assert.equal(launched.length, 17, "error releases a slot");
const timeout = [...timers.values()][0]; timeout();
assert.ok(launched[1].child.killed, "deadline kills a stuck relay");
await cleanup();
assert.ok(subscriberSignal.aborted); assert.equal(disposed, 3); assert.equal(timers.size, 0);
assert.ok(launched.slice(2).every(item => item.child.killed));
const count = launched.length; hooks.get("execute.before")(input); assert.equal(launched.length, count, "unloaded plugin cannot spawn");
assert.ok(warnings.every(value => !value.includes("PRIVATE")));
console.log("OpenCode adapter syntax, lifecycle, privacy, fixed executable, process bounds, errors, timeout and cleanup: PASS");
