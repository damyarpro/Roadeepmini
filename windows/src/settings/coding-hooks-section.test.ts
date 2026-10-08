import {beforeEach,describe,expect,it,vi} from "vitest";
import {codingHooksSection,hookPreviewFixtures} from "./coding-hooks-section";
import {Bridge} from "../core/bridge";
import {setLanguage} from "../core/i18n";
vi.mock("../core/bridge",()=>({IS_TAURI:true,Bridge:{codingHooksStatus:vi.fn(),codingHooksPreview:vi.fn(),codingHooksApply:vi.fn()}}));
const flush=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
beforeEach(()=>{vi.clearAllMocks();document.body.replaceChildren();setLanguage("fa");});
describe("coding hook setup",()=>{
 it("shows separate providers, native scope and permission limits behind help",()=>{
  const section=codingHooksSection(hookPreviewFixtures());document.body.append(section);
  expect(section.querySelectorAll(".coding-hook-provider")).toHaveLength(12);
  expect(section.textContent).toContain("Codex");expect(section.textContent).toContain("Cursor");
  expect(section.querySelector('#hook-project-vscode')).not.toBeNull();
  expect(section.textContent).not.toContain("API key");
 });
 it("requires a project folder before requesting a project preview",()=>{
  const section=codingHooksSection(hookPreviewFixtures().filter(s=>s.provider==="vscode"));document.body.append(section);
  section.querySelector<HTMLButtonElement>(".primary")!.click();
  expect(Bridge.codingHooksPreview).not.toHaveBeenCalled();expect(document.activeElement?.id).toBe("hook-project-vscode");
 });
 it("writes only the reviewed provider and fingerprint after explicit confirmation",async()=>{
  const status=hookPreviewFixtures().find(s=>s.provider==="codex")!;
  vi.mocked(Bridge.codingHooksPreview).mockResolvedValue({provider:"codex",settingsPath:"fixture",backup:"fixture.backup",fingerprint:"reviewed",diff:"+ own hook"});
  vi.mocked(Bridge.codingHooksApply).mockResolvedValue({...status,installed:true});
  const section=codingHooksSection([status]);document.body.append(section);section.querySelector<HTMLButtonElement>(".primary")!.click();await flush();
  expect(Bridge.codingHooksApply).not.toHaveBeenCalled();
  expect(section.querySelector("pre")?.textContent).toBe("+ own hook");
  section.querySelector<HTMLButtonElement>(".primary")!.click();await flush();
  expect(Bridge.codingHooksApply).toHaveBeenCalledWith("codex","reviewed",null,false);
  expect(section.textContent).toContain("اتصال به‌روز شد");
 });
 it("shows a retryable setup failure and never writes a failed preview",async()=>{
  vi.mocked(Bridge.codingHooksPreview).mockRejectedValue(new Error("hook-config-invalid"));
  const section=codingHooksSection(hookPreviewFixtures().filter(s=>s.provider==="cursor"));document.body.append(section);section.querySelector<HTMLButtonElement>(".primary")!.click();await flush();
  expect(section.textContent).toContain("پیش‌نمایش آماده نشد");expect(Bridge.codingHooksApply).not.toHaveBeenCalled();expect(section.querySelector<HTMLButtonElement>(".primary")!.disabled).toBe(false);
 });
 it("ends visible loading on status failure and offers retry",async()=>{
  vi.mocked(Bridge.codingHooksStatus).mockRejectedValue(new Error("hook-config-unreadable"));
  const section=codingHooksSection();document.body.append(section);expect(section.textContent).toContain("خواندن وضعیت");await flush();expect(section.textContent).not.toContain("خواندن وضعیت");expect(section.textContent).toContain("دوباره تلاش کن");expect(section.querySelector('[aria-busy="true"]')).toBeNull();
 });
 it("disables both confirmation actions during saving and restores focus after success",async()=>{
  const status=hookPreviewFixtures().find(s=>s.provider==="codex")!;
  vi.mocked(Bridge.codingHooksPreview).mockResolvedValue({provider:"codex",settingsPath:"fixture",backup:"fixture.backup",fingerprint:"reviewed",diff:"+ own hook"});
  let finish!:(value:typeof status)=>void;vi.mocked(Bridge.codingHooksApply).mockImplementation(()=>new Promise(resolve=>finish=resolve));
  const section=codingHooksSection([status]);document.body.append(section);section.querySelector<HTMLButtonElement>(".primary")!.click();await flush();section.querySelector<HTMLButtonElement>(".primary")!.click();
  expect([...section.querySelectorAll<HTMLButtonElement>(".actions button")].every(b=>b.disabled)).toBe(true);expect(section.textContent).toContain("ثبت تغییرات اتصال");
  finish({...status,installed:true});await flush();expect(document.activeElement?.tagName).toBe("SUMMARY");
 });
 it("marks hooks from the previous version as needing an update and offers the reviewed update",()=>{
  const status={...hookPreviewFixtures().find(s=>s.provider==="claude")!,installed:false,legacyRelay:true};
  const section=codingHooksSection([status]);document.body.append(section);
  expect(section.querySelector(".coding-hook-summary")?.textContent).toContain("نیاز به به‌روزرسانی — نسخهٔ قبلی برنامه نصب شده است");
  expect(section.querySelector<HTMLButtonElement>(".primary")?.textContent).toBe("به‌روزرسانی اتصال");
  section.querySelector<HTMLButtonElement>(".primary")!.click();
  expect(Bridge.codingHooksPreview).toHaveBeenCalledWith("claude",null,false);expect(Bridge.codingHooksApply).not.toHaveBeenCalled();
 });
});
