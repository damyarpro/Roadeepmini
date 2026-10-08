import { afterEach, describe, expect, it, vi } from "vitest";
import { setLanguage } from "../core/i18n";
import type { RoadeepModel } from "../core/bridge";
import { createChatModelPicker, type ChatModelSnapshot } from "./chat-models";

const model = (id: string, extra: Partial<RoadeepModel> = {}): RoadeepModel => ({
  id, displayName: id, description: null, provider: null, isDefault: false,
  vision: false, fileInput: false, reasoning: true, webSearch: true, deepResearch: false, tools: true, ...extra,
});
const pickers:Array<ReturnType<typeof createChatModelPicker>>=[];
afterEach(()=>{pickers.splice(0).forEach(p=>p.dispose());document.body.replaceChildren();});
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
function setup(load = vi.fn(async () => [model("text-one", { isDefault: true }), model("text-two")])) {
  setLanguage("en");
  const snapshot: ChatModelSnapshot = { signedIn: true, busy: false, selected: "text-one", pinnedModel: null };
  const change = vi.fn(async (id: string) => { snapshot.selected = id; });
  const log = vi.fn();
  const picker = createChatModelPicker({ snapshot: () => snapshot, load, change, log });
  pickers.push(picker);document.body.append(picker.el,picker.hint);
  const select = picker.el.querySelector<HTMLButtonElement>(".chat-model-pick")!;
  picker.sync();
  return { snapshot, picker, select, load, change, log };
}
function choose(select: HTMLButtonElement, value: string) {
  select.click();
  [...document.querySelectorAll<HTMLButtonElement>(".chat-model-option")].find(b=>b.dataset.modelId===value)?.click();
}

describe("chat text model picker", () => {
  it("shows the historical model separately and allows choosing the saved new-chat preference", async () => {
    const { picker, select, snapshot, change } = setup();
    await flush();
    snapshot.hasThread = true;
    snapshot.currentModel = "text-two";
    picker.sync();
    expect(select.dataset.modelId).toBe("text-two");
    expect(picker.hint.textContent).toContain("This chat: text-two");
    choose(select, "text-one");
    await flush();
    expect(change).toHaveBeenCalledExactlyOnceWith("text-one");
  });

  it("does not claim a default catalogue entry is the unknown historical model", async () => {
    const { picker, snapshot } = setup();
    await flush();
    snapshot.hasThread = true;
    snapshot.currentModel = null;
    picker.sync();
    expect(picker.hint.querySelector(".chat-model-current")?.textContent).toBe("This chat: Unknown model");
  });

  it("loads once, displays the server default, and commits only after a checked change", async () => {
    const { picker, select, change } = setup();
    expect(select.disabled).toBe(true);
    await flush();
    expect(select.disabled).toBe(false);
    select.click();
    expect(document.querySelector(".chat-model-option")?.textContent).toContain("Automatic · text-one");
    select.click();
    choose(select, "text-two");
    expect(picker.busy).toBe(true);
    expect(select.dataset.modelId).toBe("text-one");
    await flush();
    expect(change).toHaveBeenCalledExactlyOnceWith("text-two");
    expect(select.dataset.modelId).toBe("text-two");
    expect(picker.busy).toBe(false);
  });

  it("restores the prior selection and announces persistence failure", async () => {
    const { select, picker, change, log } = setup();
    await flush();
    change.mockRejectedValueOnce(new Error("disk full"));
    choose(select, "text-two");
    await flush();
    expect(select.dataset.modelId).toBe("text-one");
    expect(picker.hint.querySelector('[role="status"]')?.textContent).toMatch("Could not change");
    expect(log).toHaveBeenCalledWith("chat: model change failed");
  });

  it("locks pinned agents and in-flight turns even for synthetic change events", async () => {
    const { select, picker, snapshot, change } = setup();
    await flush();
    snapshot.pinnedModel = "text-two";
    picker.sync();
    expect(select.disabled).toBe(true);
    expect(select.dataset.modelId).toBe("text-two");
    choose(select, "text-one");
    expect(change).not.toHaveBeenCalled();
    snapshot.pinnedModel = null;
    snapshot.busy = true;
    picker.sync();
    choose(select, "text-two");
    expect(change).not.toHaveBeenCalled();
  });

  it("offers retry on catalog failure and does not invent selectable model IDs", async () => {
    const load = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce([model("available")]);
    const { select, picker } = setup(load);
    await flush();
    expect(select.disabled).toBe(true);
    expect(picker.hint.querySelector('[role="status"]')?.textContent).toBe("Models unavailable");
    const retry = picker.el.querySelector<HTMLButtonElement>(".chat-model-retry")!;
    expect(retry.hidden).toBe(false);
    retry.click();
    await flush();
    select.click();
    expect([...document.querySelectorAll<HTMLElement>(".chat-model-option")].some(b=>b.dataset.modelId==="text-one")).toBe(false);
    select.click();
    expect(select.dataset.modelId).toBe("text-one");
    expect(retry.hidden).toBe(true);
    expect(select.disabled).toBe(false);
    expect(picker.hint.querySelector('[role="status"]')?.textContent).toBe("");
  });

  it("discards results from a previous signed-in session", async () => {
    let finish!: (models: RoadeepModel[]) => void;
    const load = vi.fn(() => new Promise<RoadeepModel[]>((resolve) => { finish = resolve; }));
    const { picker, snapshot } = setup(load);
    snapshot.signedIn = false;
    picker.sync();
    finish([model("private-old")]);
    await flush();
    expect(picker.el.hidden).toBe(true);
    expect(document.body.textContent).not.toContain("private-old");
    snapshot.signedIn = true;
    picker.sync();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("uses textContent for server labels and supports Persian direction", async () => {
    const { picker, select } = setup(vi.fn(async () => [model("safe", { displayName: '<img src=x onerror="alert(1)">' })]));
    await flush();
    expect(picker.el.querySelector("img")).toBeNull();
    select.click();
    expect(document.querySelectorAll(".chat-model-option")[1].textContent).toContain("<img");
    expect(document.querySelector("img")).toBeNull();
    select.click();
    setLanguage("fa");
    picker.sync();
    expect(select.dir).toBe("rtl");
    expect(select.getAttribute("aria-label")).toContain("مدل گفت‌وگوی تازه");
    setLanguage("en");
  });
  it("searches catalogue labels, handles keyboard selection, Escape and outside clicks", async()=>{
    const {select,change}=setup();await flush();select.click();
    const search=document.querySelector<HTMLInputElement>(".chat-model-search")!;
    expect(document.activeElement).toBe(search);
    search.value="text-two";search.dispatchEvent(new Event("input"));
    expect(document.querySelectorAll(".chat-model-option")).toHaveLength(1);
    search.dispatchEvent(new KeyboardEvent("keydown",{key:"ArrowDown",bubbles:true}));
    expect(document.activeElement?.getAttribute("data-model-id")).toBe("text-two");
    search.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}));
    expect(select.getAttribute("aria-expanded")).toBe("false");expect(document.activeElement).toBe(select);
    select.click();document.body.dispatchEvent(new Event("pointerdown",{bubbles:true}));
    expect(select.getAttribute("aria-expanded")).toBe("false");
    select.click();const again=document.querySelector<HTMLInputElement>(".chat-model-search")!;
    again.value="text-two";again.dispatchEvent(new Event("input"));
    again.dispatchEvent(new KeyboardEvent("keydown",{key:"Enter",bubbles:true}));await flush();
    expect(change).toHaveBeenCalledExactlyOnceWith("text-two");
  });
  it("keeps the new-chat explanation behind an accessible help disclosure",async()=>{
    const {select,picker}=setup();await flush();
    expect(picker.hint.textContent).not.toContain("Changing the model");select.click();
    const help=document.querySelector<HTMLButtonElement>(".chat-model-help")!;
    const paragraph=document.querySelector<HTMLElement>(".chat-model-explanation")!;
    expect(paragraph.hidden).toBe(true);help.click();expect(paragraph.hidden).toBe(false);
    expect(help.getAttribute("aria-expanded")).toBe("true");expect(paragraph.textContent).toContain("history");
  });
  it("rejects stale option clicks after a turn starts and cleans up an open popover",async()=>{
    const {select,picker,snapshot,change}=setup();await flush();select.click();
    const stale=[...document.querySelectorAll<HTMLButtonElement>(".chat-model-option")].find(b=>b.dataset.modelId==="text-two")!;
    snapshot.busy=true;picker.sync();stale.click();expect(change).not.toHaveBeenCalled();
    expect(document.querySelector(".chat-model-popover")).toBeNull();
    snapshot.busy=false;picker.sync();select.click();picker.dispose();
    expect(document.querySelector(".chat-model-popover")).toBeNull();
    select.click();expect(select.getAttribute("aria-expanded")).toBe("false");
  });

});
