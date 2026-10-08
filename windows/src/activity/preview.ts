import { Activity } from "./store";
import { Monitor } from "./monitor";
import { State } from "../core/state";

/** Fabricated evidence only, enabled exclusively by the plain-browser entry. */
export function seedActivityPreview(search: string) {
  const q = new URLSearchParams(search);
  if (q.get("view") !== "activity") return;
  const scene = q.get("activity") ?? "live";
  State.settings.observeCodex = scene !== "off";
  Monitor.error = scene === "error";
  if (["off", "empty", "error"].includes(scene)) return;
  const at = Date.now();
  if (scene === "history") State.settings.retainCodingHistory = true;
  const events = [
    { id:"demo-session", sessionId:"demo-codex", harness:"codex", kind:"session", at:at-6000, cwd:"D:\\Projects\\roadeep", title:"Roadeep · activity view" },
    { id:"demo-edit-start", sessionId:"demo-codex", harness:"codex", kind:"tool", at:at-5500, tool:"apply_patch", phase:"started", callId:"edit-1", files:["src/activity.ts"], patch:"*** Begin Patch\n*** Update File: src/activity.ts\n@@\n-const visible = false;\n+const visible = enabled;\n+renderActivity(session);\n*** End Patch" },
    { id:"demo-edit", sessionId:"demo-codex", harness:"codex", kind:"tool", at:at-5000, tool:"apply_patch", phase:"completed", callId:"edit-1", exitCode:0, output:"Success. Updated the following files:\nM src/activity.ts" },
    { id:"demo-test-start", sessionId:"demo-codex", harness:"codex", kind:"tool", at:at-4000, tool:"exec_command", phase:"started", callId:"test-1", command:"npm test -- activity" },
    { id:"demo-test-end", sessionId:"demo-codex", harness:"codex", kind:"tool", at:at-3000, tool:"exec_command", phase:"completed", callId:"test-1", output:"Test Files  2 passed (2)\nTests  12 passed (12)", exitCode:0 },
    { id:"demo-second", sessionId:"demo-claude", harness:"claude", kind:"session", at:at-2000, cwd:scene === "conflict" ? "D:\\Projects\\roadeep" : "D:\\Projects\\website", title:"Website · Claude" },
    { id:"demo-second-tool", sessionId:"demo-claude", harness:"claude", kind:"tool", phase:"started", tool:"Read", at:at-1000 },
    { id:"demo-finish", sessionId:"demo-codex", harness:"codex", kind:"finished", at },
  ];
  for (const event of events) if (scene !== "conflict" || event.id !== "demo-finish") Activity.ingest(event);
  if (scene === "usage" || scene === "history") Activity.ingest({ id:"demo-usage",sessionId:"demo-codex",harness:"codex",kind:"usage",at,
    context:{usedTokens:64000,limitTokens:256000,usedPercent:25,model:"Observed model"},
    usage:{primary:{usedPercent:36,resetsAt:at+3600000},secondary:{usedPercent:18,resetsAt:at+86400000}} });
  if (scene === "history") {
    const retained = Activity.retainedEvents();
    Activity.clear();
    Activity.restore(retained);
  }
  if (scene === "stale") Activity.ingest({id:"demo-new-edit",sessionId:"demo-codex",harness:"codex",kind:"tool",tool:"apply_patch",phase:"completed",at:at+1000,exitCode:0,files:["src/activity.ts"],patch:"-const ready = false;\n+const ready = true;"});
  if (scene === "conflict") {
    Activity.ingest({id:"demo-overlap",sessionId:"demo-claude",harness:"claude",kind:"tool",tool:"Edit",phase:"completed",at:at+1000,exitCode:0,files:["src/activity.ts"]});
    Activity.ingest({id:"demo-current",sessionId:"demo-codex",harness:"codex",kind:"tool",tool:"Read",phase:"started",at:at+2000});
  }
}
