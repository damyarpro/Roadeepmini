import { Bridge, IS_TAURI, type RoadeepModel } from "../core/bridge";
import { State } from "../core/state";
import { seedChatModelPreviewThread } from "./chat";

/** Fabricated fixtures exist only in the explicit plain-browser preview. */
export function seedCodingPowerPreview(search: string) {
  if (IS_TAURI) return;
  const params = new URLSearchParams(search);
  const scene = params.get("models");
  if (scene && ["ready", "failure", "busy", "pinned", "historical", "loading"].includes(scene)) {
    const models: RoadeepModel[] = ["Example Text One", "Example Text Two"].map((displayName, i) => ({
      id:`example-text-${i+1}`,displayName,description:null,provider:null,isDefault:i===0,
      vision:false,fileInput:false,reasoning:true,webSearch:true,deepResearch:false,tools:true,
    }));
    State.roadeep.signedIn = true;
    Bridge.roadeepSession = async () => ({signedIn:true,user:null,reason:null});
    State.settings.model = models[1].id;
    Bridge.roadeepModels = async () => {
      if (scene === "failure") throw new Error("Fabricated catalogue failure");
      if (scene === "loading") return new Promise(() => {});
      return models;
    };
    Bridge.chatModelSet = async (model) => model;
    Bridge.roadeepBalance = async () => ({units:2400,plan:"Preview"});
    Bridge.roadeepAgents = async () => [];
    Bridge.roadeepAgentCatalog = async () => ({exclusive:[],public:[]});
    Bridge.localAgentsList = async () => State.roadeep.localAgents;
    Bridge.chatToolsState = async () => ({available:false,on:false,count:0,defaultOn:false});
    Bridge.chatTurnState = async () => ({busy:scene === "busy",turn:scene === "busy" ? 42 : null});
    if (scene === "pinned") {
      State.settings.chatAgent = "local:preview";
      State.roadeep.localAgents = [{ id:"preview",name:"Example coding agent",instructions:"Preview",model:models[0].id,
        webSearch:false,baseAgentId:null,description:"Preview",color:"",starterPrompts:[],createdAt:0,updatedAt:0 }];
    }
    if (scene === "historical") seedChatModelPreviewThread(models[0].id);
  }
  if (params.get("view") === "activity" && ["git", "handoff"].includes(params.get("activity") ?? "")) {
    Bridge.codingGitInspect = async (cwd) => ({root:cwd,head:"main",at:Date.now(),files:[{path:"src/activity.ts",status:"M"}],
      patch:"diff --git a/src/activity.ts b/src/activity.ts\n@@ -1 +1 @@\n-const enabled = false;\n+const enabled = true;",truncated:false});
    Bridge.codingHandoffAgents = async () => [{id:"codex",name:"Codex (preview)",executable:"C:\\Example\\codex.exe"}];
    Bridge.codingHandoff = async () => ({path:"C:\\Example\\handoff.md",agent:"codex"});
  }
}
