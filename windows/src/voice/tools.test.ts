import { describe, expect, it, vi } from "vitest";
import type { FocusState, PlannerData, PlannerHabit, PlannerTask } from "../core/bridge-planner";
import type { ClockNow } from "../core/bridge-assistant";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { TOOL_SPECS, parseAppToolArgs, parseToolArgs, toolSpec, validate } from "./schema";
import { ASK_TIMEOUT_MS, ISLAND_VOICE_VIEWS, MAX_APP_TOOLS, MAX_OUTPUT, applySettingsPatch, validAppTools, createToolRuntime, matchTitle, titleScore, type ToolDeps, type ToolHost } from "./tools";

let memoryView = {
  enabled: true,
  facts: [{ id: "f1", text: "جواب کوتاه دوست دارد", category: "preference", updatedAt: 2 }, { id: "f2", text: "شب‌ها کار می‌کند", category: "habit", updatedAt: 1 }],
  stats: { toolCounts: { add_task: 7, list_notes: 2 }, hourHistogram: Array.from({ length: 24 }, (_, h) => (h === 21 ? 5 : 0)), recent: [{ at: 1, tool: "add_task", summary: "افزودن کار «نان»" }] },
};
const NOW = new Date(2026, 9, 7, 10, 0).getTime();
const TODAY = "2026-10-07";
const CLOCK: ClockNow = { epochMs: NOW, isoLocal: "2026-10-07T14:05:00+03:30", utcOffsetMinutes: 210, timezoneName: "Iran Standard Time",
  jalali: { y: 1405, m: 7, d: 15, monthNameFa: "مهر" }, gregorian: { y: 2026, m: 10, d: 7 }, weekdayFa: "چهارشنبه", weekdayEn: "Wednesday",
  source: "online", onlineHost: "api.openai.com", skewSeconds: 0 };
const task = (id: string, title: string, extra: Partial<PlannerTask> = {}): PlannerTask =>
  ({ id, title, note: "", done: false, due: null, createdAt: 1, updatedAt: 1, doneAt: null, ...extra });
const habit = (id: string, title: string, log: string[] = []): PlannerHabit =>
  ({ id, title, icon: "water", days: "daily", nudge: null, log, createdAt: 1 });
const focus: FocusState = { phase: "focus", endsAt: NOW + 12 * 60_000, remainingMs: null, round: 2, roundsDoneToday: 1, startedAt: NOW, dayKey: TODAY };

function fixture() {
  const data: PlannerData = {
    version: 1,
    tasks: [task("t1", "خرید نان و شیر", { due: TODAY }), task("t2", "تماس با پشتیبانی بانک", { due: "2026-10-01" }),
      task("t3", "فرستادن گزارش هفتگی"), task("t4", "فرستادن گزارش ماهانه"), task("t5", "پرداخت قبض برق", { done: true, due: TODAY })],
    notes: [{ id: "n1", text: "رمز وای‌فای", pinned: false, createdAt: 1, updatedAt: 5 }, { id: "n2", text: "سنجاق", pinned: true, createdAt: 1, updatedAt: 1 }],
    reminders: [{ id: "r1", title: "قرص", at: NOW + 20 * 60_000, repeat: "none", enabled: true, lastFiredAt: null, snoozedUntil: null, createdAt: 1 },
      { id: "r2", title: "خاموش", at: NOW + 60_000, repeat: "none", enabled: false, lastFiredAt: null, snoozedUntil: null, createdAt: 1 }],
    habits: [habit("h1", "آب بنوش", ["2026-10-05", "2026-10-06"]), habit("h2", "کشش", [TODAY])],
    focus,
  };
  let settings: Settings = { ...DEFAULT_SETTINGS };
  const planner = {
    get: vi.fn(async () => data),
    taskAdd: vi.fn(async (title: string, due: string | null = null) => task("new", title, { due })),
    taskUpdate: vi.fn(async (id: string, patch: Partial<PlannerTask>) => ({ ...data.tasks.find(t => t.id === id)!, ...patch })),
    taskDelete: vi.fn(async () => undefined),
    noteAdd: vi.fn(async (text: string) => ({ id: "n", text, pinned: false, createdAt: 1, updatedAt: 1 })),
    noteUpdate: vi.fn(async (id: string, patch: { text?: string }) => ({ ...data.notes.find(n => n.id === id)!, ...patch })),
    noteDelete: vi.fn(async () => undefined),
    reminderAdd: vi.fn(async (title: string, at: number) => ({ ...data.reminders[0], id: "r", title, at })),
    habitCheck: vi.fn(async (id: string) => data.habits.find(h => h.id === id)!),
    focusStart: vi.fn(async () => focus), focusPause: vi.fn(async () => ({ ...focus, phase: "paused" as const })),
    focusResume: vi.fn(async () => focus), focusSkip: vi.fn(async () => focus), focusStop: vi.fn(async () => ({ ...focus, phase: "idle" as const })),
    focusGet: vi.fn(async () => focus),
  };
  const deps: ToolDeps = {
    planner: planner as unknown as ToolDeps["planner"],
    settings: () => settings,
    saveSettings: vi.fn(async (next: Settings) => { settings = next; }),
    openSettings: vi.fn(async () => undefined),
    now: () => NOW,
    appTool: vi.fn(async (name: string, args: Record<string, unknown>) => `native ${name} ${JSON.stringify(args)}`),
    memory: {
      get: vi.fn(async () => memoryView),
      remember: vi.fn(async (_text: string, _category: string) => ({ id: "f" })),
      forget: vi.fn(async (_query: string) => 2),
      record: vi.fn(async (_tool: string, _summary: string) => undefined),
    },
    clock: vi.fn(async () => CLOCK),
  };
  const host: ToolHost = {
    showView: vi.fn(), ask: vi.fn(async () => "پاسخ"), setCharacter: vi.fn(),
    characterOptions: () => ({
      shapes: [{ id: "blob", label: { fa: "حبابی", en: "Blob" } }, { id: "star", label: { fa: "ستاره", en: "Star" } }],
      colors: [{ id: "sky", label: { fa: "آسمانی", en: "Sky" } }],
      expressions: [{ id: "happy", label: "شاد" }],
    }),
  };
  const raw = createToolRuntime(host, deps);
  // Most tests only read the text; `raw` keeps the success flag.
  const rt = { ...raw, run: async (...a: Parameters<typeof raw.run>) => { const r = await raw.run(...a); return typeof r === "string" ? r : r.output; } };
  return { rt, raw, data, deps, host, planner, settings: () => settings };
}

describe("schema", () => {
  it("covers every declared tool with an object schema", () => {
    expect(TOOL_SPECS.length).toBeGreaterThan(10);
    for (const spec of TOOL_SPECS) expect(spec.parameters.type).toBe("object");
  });
  it("validates types, enums, bounds, lengths, patterns and extra keys", () => {
    expect(parseToolArgs("add_task", '{"title":"x","due":"2026-10-07"}')).toEqual({ ok: true, args: { title: "x", due: "2026-10-07" } });
    expect(parseToolArgs("add_task", '{"title":"x","due":"7 Oct"}').ok).toBe(false);
    expect(parseToolArgs("add_task", '{"due":"2026-10-07"}').ok).toBe(false);
    expect(parseToolArgs("add_task", "[]").ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"soundVolume":2}').ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"focusMinutes":2.5}').ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"language":"de"}').ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"__proto__":1}').ok).toBe(false);
    expect(parseToolArgs("get_settings", "").ok).toBe(true);
    expect(parseToolArgs("nope", "{}").ok).toBe(false);
    expect(parseToolArgs("add_note", 7).ok).toBe(false);
    expect(validate("ab", { type: "string", maxLength: 1 })).toContain("too long");
  });
});

describe("app tools", () => {
  const tools = [
    { name: "app__github__list_issues", title: "List issues", server: "GitHub", readOnly: true },
    { name: "app__github__create_issue", title: "Create issue", server: "GitHub", readOnly: false },
  ];

  it("validates the appTools list from voice_start", () => {
    expect(validAppTools([...tools, tools[0], { name: "bad name", title: "x", server: "y", readOnly: true },
      { name: "app__x", title: "x", server: "y" }, { name: "planner_add", title: "x", server: "y", readOnly: true }, null])).toEqual(tools);
    expect(validAppTools("nope")).toEqual([]);
    expect(validAppTools(Array.from({ length: 80 }, (_, i) => ({ name: `app__t${i}`, title: "t", server: "s", readOnly: true })))).toHaveLength(MAX_APP_TOOLS);
    expect(validAppTools([{ name: "app__t", title: "  ", server: "", readOnly: true }])[0]).toMatchObject({ title: "app__t", server: "app" });
  });

  it("validates app tool arguments as a JSON object of at most 16 KB", () => {
    expect(parseAppToolArgs('{"repo":"a"}')).toEqual({ ok: true, args: { repo: "a" } });
    expect(parseAppToolArgs("").ok).toBe(true);
    for (const bad of ["[]", "1", "{oops", JSON.stringify({ x: "x".repeat(17 * 1024) }), 5]) expect(parseAppToolArgs(bad).ok).toBe(false);
  });

  it("knows, summarises and runs the session's app tools", async () => {
    const { rt, deps } = fixture();
    expect(rt.knows("app__github__list_issues")).toBe(false);
    rt.setAppTools(tools);
    expect(rt.knows("app__github__list_issues")).toBe(true); expect(rt.knows("app__other")).toBe(false); expect(rt.knows("add_task")).toBe(true);
    expect(rt.isMutating("app__github__list_issues")).toBe(false); expect(rt.isMutating("app__github__create_issue")).toBe(true);
    expect(rt.summarize("app__github__create_issue", { title: "Bug" })).toBe('اجرای «Create issue» از GitHub — {"title":"Bug"}');
    expect(await rt.prepare("app__github__create_issue", { title: "Bug" })).toEqual({ ok: true, args: { title: "Bug" }, summary: expect.stringContaining("Create issue") });
    expect(await rt.run("app__github__list_issues", {})).toBe("native app__github__list_issues {}");
    expect(deps.appTool).toHaveBeenLastCalledWith("app__github__list_issues", {}, false);
    expect(await rt.run("app__github__create_issue", { title: "Bug" })).toContain("بدون تأیید");
    await rt.run("app__github__create_issue", { title: "Bug" }, undefined, true);
    expect(deps.appTool).toHaveBeenLastCalledWith("app__github__create_issue", { title: "Bug" }, true);
    vi.mocked(deps.appTool).mockRejectedValueOnce("E_MCPC_FAILED");
    expect(await rt.run("app__github__list_issues", {})).toMatch(/^انجام نشد/);
    rt.setAppTools(null);
    expect(rt.knows("app__github__list_issues")).toBe(false);
    expect(await rt.run("app__github__list_issues", {})).toContain("در دسترس نیست");
    expect((await rt.prepare("app__github__list_issues", {})).ok).toBe(false);
  });
});

describe("title matching", () => {
  it("scores exact, contained and word matches", () => {
    expect(titleScore("خرید نان و شیر", "خريد نان و شير")).toBe(1);
    expect(titleScore("نان", "خرید نان و شیر")).toBe(0.9);
    expect(titleScore("تماس بانک", "تماس با پشتیبانی بانک")).toBeGreaterThan(0.5);
    expect(titleScore("ماشین", "خرید نان")).toBe(0);
  });
  it("returns one, many or none", () => {
    const items = ["فرستادن گزارش هفتگی", "فرستادن گزارش ماهانه", "خرید نان"];
    expect(matchTitle(items, "نان", x => x)).toEqual({ kind: "one", item: "خرید نان" });
    expect(matchTitle(items, "گزارش", x => x)).toEqual({ kind: "many", items: items.slice(0, 2) });
    expect(matchTitle(items, "گزارش ماهانه", x => x)).toEqual({ kind: "one", item: "فرستادن گزارش ماهانه" });
    expect(matchTitle(items, "دوچرخه", x => x)).toEqual({ kind: "none" });
  });
});

describe("notes and memory tools", () => {
  const notesFixture = () => {
    const f = fixture();
    f.data.notes = [
      { id: "n1", text: "رمز وای‌فای مهمان روی برگهٔ زرد است", pinned: false, createdAt: 1, updatedAt: 5 },
      { id: "n2", text: "ایده برای جلسهٔ شنبه", pinned: true, createdAt: 1, updatedAt: 1 },
      { id: "n3", text: "ایده برای سفر تابستان", pinned: false, createdAt: 1, updatedAt: 9 },
      ...Array.from({ length: 25 }, (_, i) => ({ id: `x${i}`, text: `خرید شمارهٔ ${i}`, pinned: false, createdAt: 1, updatedAt: 0 })),
    ];
    return f;
  };

  it("lists notes by words, newest first, at most 20, ids hidden", async () => {
    const { rt } = notesFixture();
    const ideas = await rt.run("list_notes", { query: "ایده" });
    expect(ideas).toContain("2 یادداشت"); expect(ideas.indexOf("سفر")).toBeLessThan(ideas.indexOf("شنبه"));
    expect(await rt.run("list_notes", { query: "وای‌فای زرد" })).toContain("رمز");
    expect(await rt.run("list_notes", { query: "دوچرخه" })).toContain("پیدا نشد");
    const all = await rt.run("list_notes", {});
    expect(all).toContain("و 8 یادداشت دیگر"); expect(all).not.toMatch(/\bx\d|n1|n2/);
  });

  it("updates a note found by its words, previewing the new text", async () => {
    const { rt, planner } = notesFixture();
    const prepared = await rt.prepare("update_note", { query: "وای‌فای", text: "رمز تغییر کرد" });
    expect(prepared).toMatchObject({ ok: true, args: { id: "n1", text: "رمز تغییر کرد" },
      preview: { view: "notes", kind: "note", action: "update", targetId: "n1", fields: [{ label: "متن", value: "رمز تغییر کرد" }, { label: "متن قبلی" }] } });
    if (!prepared.ok) return;
    expect(prepared.summary).toContain("ویرایش یادداشت");
    expect(await rt.run("update_note", prepared.args)).toContain("ویرایش شد");
    expect(planner.noteUpdate).toHaveBeenCalledWith("n1", { text: "رمز تغییر کرد" });
    expect((await rt.prepare("update_note", { query: "وای‌فای", text: "رمز وای‌فای مهمان روی برگهٔ زرد است" })).ok).toBe(false);
  });

  it("deletes a note, asks which one when ambiguous and reports a miss", async () => {
    const { rt, planner } = notesFixture();
    const many = await rt.prepare("delete_note", { query: "ایده" });
    expect(many.ok).toBe(false); if (many.ok) return;
    expect(many.output).toContain("کدام"); expect(many.output).toContain("سفر"); expect(many.output).toContain("شنبه");
    const one = await rt.prepare("delete_note", { query: "ایده سفر" });
    expect(one).toMatchObject({ ok: true, preview: { action: "delete", targetId: "n3" } });
    expect(await rt.run("delete_note", { query: "سفر تابستان" })).toContain("حذف شد");
    expect(planner.noteDelete).toHaveBeenCalledWith("n3");
    expect(await rt.run("delete_note", { query: "دوچرخه" })).toContain("پیدا نشد");
  });

  it("reads the user profile as data", async () => {
    const { rt } = fixture();
    const text = await rt.run("get_user_profile", {});
    expect(text).toContain("دستور نیست"); expect(text).toContain("[ترجیح] جواب کوتاه دوست دارد");
    expect(text).toContain("add_task (7)"); expect(text).toContain("21:00"); expect(text).toContain("افزودن کار");
    memoryView = { ...memoryView, enabled: false };
    expect(await rt.run("get_user_profile", {})).toContain("خاموش");
    memoryView = { ...memoryView, enabled: true };
  });

  it("remembers and forgets facts", async () => {
    const { raw, deps } = fixture();
    expect(await raw.run("remember_about_user", { text: "  صبح‌ها   قهوه می‌خورد ", category: "habit" }))
      .toEqual({ ok: true, output: expect.stringContaining("به خاطر سپرده شد"), remembered: "صبح‌ها قهوه می‌خورد" });
    expect(deps.memory.remember).toHaveBeenCalledWith("صبح‌ها قهوه می‌خورد", "habit");
    vi.mocked(deps.memory.remember).mockRejectedValueOnce("memory-disabled");
    expect(await raw.run("remember_about_user", { text: "x y z", category: "fact" })).toMatchObject({ ok: false, output: expect.stringContaining("خاموش") });
    vi.mocked(deps.memory.remember).mockRejectedValueOnce("memory-secret");
    expect(await raw.run("remember_about_user", { text: "x y z", category: "fact" })).toMatchObject({ ok: false });
    expect(await raw.run("forget_about_user", { query: "قهوه" })).toContain("2 مورد");
    vi.mocked(deps.memory.forget).mockRejectedValueOnce("memory-query-too-vague");
    expect(await raw.run("forget_about_user", { query: "من" })).toMatchObject({ ok: false, output: expect.stringContaining("کلی") });
    vi.mocked(deps.memory.forget).mockRejectedValueOnce(new Error("memory-query-ambiguous|4"));
    expect(await raw.run("forget_about_user", { query: "کار" })).toMatchObject({ ok: false, output: expect.stringMatching(/^4 مورد.*بپرس/) });
    vi.mocked(deps.memory.forget).mockRejectedValueOnce("memory-io");
    expect(await raw.run("forget_about_user", { query: "کار" })).toMatchObject({ ok: false, output: expect.stringMatching(/^انجام نشد/) });
    vi.mocked(deps.memory.forget).mockResolvedValueOnce(0);
    expect(await raw.run("forget_about_user", { query: "چای" })).toContain("نبود");
  });

  it("records executed tools without waiting or throwing, and skips memory tools", async () => {
    const { raw, deps } = fixture();
    raw.record("add_task", { title: "نان" }); raw.record("remember_about_user", { text: "x" });
    await Promise.resolve(); await Promise.resolve();
    expect(deps.memory.record).toHaveBeenCalledTimes(1);
    expect(deps.memory.record).toHaveBeenCalledWith("add_task", "افزودن کار «نان»");
    raw.setAppTools([{ name: "app__gh__create", title: "Create issue", server: "GitHub", readOnly: false }]);
    raw.record("app__gh__create", { secret: "token-123" }); await Promise.resolve(); await Promise.resolve();
    expect(vi.mocked(deps.memory.record).mock.calls.at(-1)).toEqual(["app__gh__create", "اجرای «Create issue» از GitHub"]);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(deps.memory.record).mockRejectedValueOnce(new Error("private text"));
    expect(() => raw.record("add_note", { text: "x" })).not.toThrow();
    await new Promise(r => setTimeout(r, 0));
    expect(JSON.stringify(warn.mock.calls)).not.toContain("private text");
    warn.mockRestore();
  });
});

describe("tool runtime", () => {
  it("shows exactly the island views tools.json offers", () => {
    expect([...ISLAND_VOICE_VIEWS].sort()).toEqual([...(toolSpec("show_view")!.parameters.properties!.view.enum as string[])].sort());
  });

  it("previews planner changes with their resolved targets", async () => {
    const { rt } = fixture();
    expect(await rt.prepare("add_task", { title: "بلیت", due: "2026-10-09" })).toMatchObject({ preview: {
      view: "tasks", kind: "task", action: "create", fields: [{ label: "عنوان", value: "بلیت" }, { label: "موعد", value: "2026-10-09" }] } });
    expect(await rt.prepare("complete_task", { query: "نان" })).toMatchObject({ preview: { view: "tasks", action: "update", targetId: "t1", fields: [{ value: "خرید نان و شیر" }, { value: "انجام‌شده" }] } });
    expect(await rt.prepare("delete_task", { query: "بانک" })).toMatchObject({ preview: { action: "delete", targetId: "t2" } });
    expect(await rt.prepare("add_note", { text: "خط ۱\nخط ۲" })).toMatchObject({ preview: { view: "notes", kind: "note", fields: [{ value: "خط ۱\nخط ۲" }] } });
    expect(await rt.prepare("add_reminder", { title: "جلسه", inMinutes: 30 })).toMatchObject({ preview: { view: "reminders", fields: [{ value: "جلسه" }, { label: "زمان", value: "2026-10-07 10:30" }] } });
    expect(await rt.prepare("log_habit", { query: "آب" })).toMatchObject({ preview: { view: "habits", kind: "habit", targetId: "h1" } });
    expect(await rt.prepare("control_focus", { action: "start", minutes: 25 })).toMatchObject({ preview: { view: "focus", kind: "focus", fields: [{ value: "شروع تایمر تمرکز" }, { value: "25 دقیقه" }] } });
    const settings = await rt.prepare("update_settings", { soundEnabled: false });
    expect(settings.ok && settings.preview).toBeUndefined();
  });

  it("knows which tools mutate (unknown ones count as mutating)", () => {
    const { rt } = fixture();
    expect(rt.isMutating("add_task")).toBe(true); expect(rt.isMutating("list_tasks")).toBe(false); expect(rt.isMutating("whatever")).toBe(true);
  });

  it("lists tasks by filter with due dates", async () => {
    const { rt } = fixture();
    const today = await rt.run("list_tasks", { filter: "today" });
    expect(today).toContain("خرید نان و شیر"); expect(today).toContain("موعد گذشته"); expect(today).not.toContain("قبض");
    expect(await rt.run("list_tasks", { filter: "done" })).toContain("قبض");
    expect(await rt.run("list_tasks", { filter: "all" })).toContain("5 کار");
  });

  it("resolves a task by fuzzy title before approval and completes it", async () => {
    const { rt, planner } = fixture();
    const prepared = await rt.prepare("complete_task", { query: "نان" });
    expect(prepared).toMatchObject({ ok: true, args: { id: "t1", title: "خرید نان و شیر" }, summary: expect.stringContaining("«خرید نان و شیر»") });
    if (!prepared.ok) return;
    expect(await rt.run("complete_task", prepared.args)).toContain("انجام‌شده");
    expect(planner.taskUpdate).toHaveBeenCalledWith("t1", { done: true });
  });

  it("asks which one when the title is ambiguous and reports a miss", async () => {
    const { rt, planner } = fixture();
    const many = await rt.prepare("delete_task", { query: "گزارش" });
    expect(many.ok).toBe(false); if (many.ok) return;
    expect(many.output).toContain("کدام"); expect(many.output).toContain("هفتگی"); expect(many.output).toContain("ماهانه");
    expect(await rt.run("delete_task", { query: "دوچرخه" })).toContain("پیدا نشد");
    expect(await rt.run("complete_task", { query: "قبض برق" })).toContain("پیدا نشد"); // already done
    expect(planner.taskDelete).not.toHaveBeenCalled();
  });

  it("adds tasks, notes and reminders", async () => {
    const { rt, planner } = fixture();
    expect(await rt.run("add_task", { title: " بلیت ", due: "2026-10-09" })).toContain("«بلیت»");
    expect(planner.taskAdd).toHaveBeenCalledWith("بلیت", "2026-10-09");
    expect((await rt.prepare("add_task", { title: "x", due: "2026-02-30" })).ok).toBe(false);
    expect(await rt.run("add_note", { text: "یادداشت" })).toContain("اضافه شد");
    const reminder = await rt.prepare("add_reminder", { title: "جلسه", inMinutes: 30 });
    expect(reminder).toMatchObject({ ok: true, args: { title: "جلسه", atMs: NOW + 30 * 60_000 } });
    expect(await rt.run("add_reminder", { title: "جلسه", at: "2026-10-07T18:30" })).toContain("2026-10-07 18:30");
    expect(planner.reminderAdd).toHaveBeenLastCalledWith("جلسه", new Date(2026, 9, 7, 18, 30).getTime());
    for (const bad of [{ title: "x" }, { title: "x", at: "2026-10-07T08:00" }, { title: "x", at: "2026-10-07T18:00", inMinutes: 5 }, { title: "x", at: "2026-13-01T10:00" }]) {
      expect((await rt.prepare("add_reminder", bad)).ok).toBe(false);
    }
  });

  it("lists reminders, notes and habits compactly", async () => {
    const { rt } = fixture();
    const reminders = await rt.run("list_reminders", {});
    expect(reminders).toContain("قرص"); expect(reminders).not.toContain("خاموش");
    const notes = await rt.run("list_notes", {});
    expect(notes.indexOf("رمز")).toBeLessThan(notes.indexOf("سنجاق")); // newest first
    expect(notes).toContain("[سنجاق]"); expect(notes).not.toContain("n1");
    const habits = await rt.run("list_habits", {});
    expect(habits).toContain("آب بنوش: امروز هنوز نه، 2 روز"); expect(habits).toContain("کشش: امروز انجام شد");
  });

  it("logs a habit once per day", async () => {
    const { rt, planner } = fixture();
    expect(await rt.run("log_habit", { query: "آب" })).toContain("ثبت شد");
    expect(planner.habitCheck).toHaveBeenCalledWith("h1", TODAY, true);
    expect(await rt.run("log_habit", { query: "کشش" })).toContain("قبلاً");
  });

  it("reads the date and time in Persian with ISO, online or from the system", async () => {
    const { rt, deps, raw } = fixture();
    expect(raw.isMutating("get_datetime")).toBe(false);
    const online = await rt.run("get_datetime", {});
    expect(online).toContain("چهارشنبه ۱۵ مهر ۱۴۰۵ (۷ اکتبر ۲۰۲۶)، ساعت ۱۴:۰۵");
    expect(online).toContain("+03:30 (Iran Standard Time)");
    expect(online).toContain("تأییدشده آنلاین از api.openai.com");
    expect(online).toContain("ISO: 2026-10-07T14:05:00+03:30");
    vi.mocked(deps.clock).mockResolvedValueOnce({ ...CLOCK, source: "system", onlineHost: null, skewSeconds: null, timezoneName: null });
    const system = await rt.run("get_datetime", {});
    expect(system).toContain("طبق ساعت سیستم");
    expect(system).not.toContain("Iran Standard Time");
  });

  it("reads and controls the focus timer", async () => {
    const { rt, planner } = fixture();
    expect(await rt.run("get_focus", {})).toContain("12 دقیقه");
    await rt.run("control_focus", { action: "start", minutes: 25 });
    expect(planner.focusStart).toHaveBeenCalledWith("focus", 25);
    expect(await rt.run("control_focus", { action: "pause" })).toContain("متوقف");
    expect(rt.summarize("control_focus", { action: "stop" })).toContain("پایان");
  });

  it("changes settings through a validated copy and summarises it in Persian", async () => {
    const { rt, deps, settings } = fixture();
    const prepared = await rt.prepare("update_settings", { soundEnabled: false, language: "en" });
    expect(prepared).toMatchObject({ ok: true, summary: expect.stringContaining("صداهای برنامه: خاموش") });
    expect(await rt.run("update_settings", { soundEnabled: false, language: "en" })).toContain("ذخیره شد");
    expect(deps.saveSettings).toHaveBeenCalledOnce();
    expect(settings()).toMatchObject({ soundEnabled: false, language: "en", characterAppearance: DEFAULT_SETTINGS.characterAppearance });
    expect(await rt.run("update_settings", { soundEnabled: false })).toContain("تغییری لازم نیست");
    expect(await rt.run("update_settings", { focusMinutes: 180 })).toContain("بین");
    expect(parseToolArgs("update_settings", '{"focusMinutes":4}').ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"roundsBeforeLongBreak":9}').ok).toBe(false);
    expect(parseToolArgs("update_settings", '{"eyeMotion":true}').ok).toBe(false);
    expect(parseToolArgs("control_focus", '{"action":"start","minutes":130}').ok).toBe(false);
    expect(deps.saveSettings).toHaveBeenCalledOnce();
  });

  it("keeps web search and reasoning exclusive and maps eye motion", () => {
    const base = { ...DEFAULT_SETTINGS, chatReasoning: true, chatWebSearch: false, eyeMotion: "calm" as const };
    const web = applySettingsPatch(base, { chatWebSearch: true });
    expect(web.ok && web.next).toMatchObject({ chatWebSearch: true, chatReasoning: false });
    expect(applySettingsPatch(base, { chatWebSearch: true, chatReasoning: true }).ok).toBe(false);
    const still = applySettingsPatch(base, { eyeMotion: "still" });
    expect(still.ok && still.next.eyeMotion).toBe("still");
    expect(still.ok && still.changes).toEqual(["حرکت چشم شخصیت: بی‌حرکت"]);
    expect(applySettingsPatch(base, { eyeMotion: "calm" }).ok).toBe(false); // already calm
    expect(applySettingsPatch(base, {}).ok).toBe(false);
  });

  it("returns settings and character options as compact JSON", async () => {
    const { rt } = fixture();
    const settings = JSON.parse(await rt.run("get_settings", {})) as Record<string, unknown>;
    expect(settings).toMatchObject({ soundEnabled: true, language: "fa", eyeMotion: DEFAULT_SETTINGS.eyeMotion }); expect(settings).not.toHaveProperty("agentColors");
    const options = JSON.parse(await rt.run("get_character_options", {})) as { shapes: { id: string; name: string }[] };
    expect(options.shapes[0]).toEqual({ id: "blob", name: "حبابی" });
  });

  it("sets the character by id or by name and refuses unknown ids", async () => {
    const { rt, host } = fixture();
    const prepared = await rt.prepare("set_character", { shape: "ستاره", color: "sky" });
    expect(prepared).toMatchObject({ ok: true, args: { shape: "star", color: "sky" }, summary: expect.stringContaining("«ستاره»") });
    if (prepared.ok) await rt.run("set_character", prepared.args);
    expect(host.setCharacter).toHaveBeenCalledWith({ shape: "star", color: "sky" });
    expect(await rt.run("set_character", { shape: "cube" })).toContain("وجود ندارد");
  });

  it("opens settings, shows views and asks Roadeep", async () => {
    const { rt, deps, host } = fixture();
    await rt.run("open_settings", { section: "planner" }); expect(deps.openSettings).toHaveBeenCalledWith("planner");
    await rt.run("show_view", { view: "planner" }); expect(host.showView).toHaveBeenCalledWith("planner");
    expect(await rt.run("ask_roadeep", { query: "هوا؟" })).toContain("پاسخ");
    vi.mocked(host.ask).mockResolvedValueOnce("x".repeat(10_000));
    expect((await rt.run("ask_roadeep", { query: "?" })).length).toBe(MAX_OUTPUT);
  });

  it("gives ask_roadeep an abort signal, a 90 s timeout and cancels it with the session", async () => {
    vi.useFakeTimers();
    try {
      const { rt, host } = fixture();
      let seen: AbortSignal | undefined;
      vi.mocked(host.ask).mockImplementation((_q, signal) => { seen = signal; return new Promise<string>(() => undefined); });
      const slow = rt.run("ask_roadeep", { query: "?" });
      await vi.advanceTimersByTimeAsync(ASK_TIMEOUT_MS);
      expect(await slow).toContain("۹۰ ثانیه"); expect(seen?.aborted).toBe(true);
      const session = new AbortController();
      const cancelled = rt.run("ask_roadeep", { query: "?" }, session.signal);
      session.abort(); expect(await cancelled).toContain("لغو"); expect(seen?.aborted).toBe(true);
      vi.mocked(host.ask).mockImplementation(async (_q, signal) => { seen = signal; return "ok"; });
      expect(await rt.run("ask_roadeep", { query: "?" })).toContain("ok");
    } finally { vi.useRealTimers(); }
  });

  it("flags refusals and failures as not ok", async () => {
    const { raw, planner } = fixture();
    expect(await raw.run("add_note", { text: "x" })).toBe("یادداشت اضافه شد.");
    expect(await raw.run("delete_task", { query: "دوچرخه" })).toMatchObject({ ok: false });
    expect(await raw.run("update_settings", { focusMinutes: 500 })).toMatchObject({ ok: false });
    planner.get.mockRejectedValueOnce(new Error("x"));
    expect(await raw.run("list_notes", {})).toMatchObject({ ok: false, output: expect.stringMatching(/^انجام نشد/) });
    raw.setAppTools([{ name: "app__s__w", title: "W", server: "S", readOnly: false }]);
    expect(await raw.run("app__s__w", {})).toMatchObject({ ok: false });
  });

  it("turns failures into a short Persian sentence instead of throwing", async () => {
    const { rt, planner, host } = fixture();
    planner.get.mockRejectedValueOnce("E_PLANNER_NOT_FOUND");
    expect(await rt.run("list_tasks", { filter: "all" })).toMatch(/^انجام نشد: /);
    vi.mocked(host.ask).mockRejectedValueOnce(new Error("offline"));
    expect(await rt.run("ask_roadeep", { query: "?" })).toContain("offline");
    planner.get.mockRejectedValueOnce(new Error("x"));
    expect((await rt.prepare("log_habit", { query: "آب" })).ok).toBe(false);
  });
});
