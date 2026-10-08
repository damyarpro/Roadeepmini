import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { helpDisclosure } from "./help";
import { sectionHead, settingRow, icon } from "./ui";
import { setLanguage } from "../core/i18n";
beforeEach(()=>setLanguage("en"));
afterEach(()=>{document.body.replaceChildren();});
describe("settings explanation disclosure",()=>{
 it("toggles unique accessible inline panels without hover",()=>{
  const one=helpDisclosure("First","Explain timer"),two=helpDisclosure("Second");document.body.append(one,two);
  const button=one.querySelector("button")!, panel=one.querySelector(".settings-help-content") as HTMLElement;
  expect(button.type).toBe("button");expect(button.getAttribute("aria-label")).toBe("Explain timer");
  expect(button.getAttribute("aria-controls")).toBe(panel.id);expect(two.querySelector(".settings-help-content")!.id).not.toBe(panel.id);
  expect(panel.hidden).toBe(true);expect(button.getAttribute("aria-expanded")).toBe("false");
  button.click();expect(panel.hidden).toBe(false);expect(button.getAttribute("aria-expanded")).toBe("true");
  button.click();expect(panel.hidden).toBe(true);
 });
 it("Escape closes and returns focus to the help button",()=>{
  const root=helpDisclosure("Details");document.body.append(root);const button=root.querySelector("button")!;
  button.click();button.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true,cancelable:true}));
  expect(button.getAttribute("aria-expanded")).toBe("false");expect(document.activeElement).toBe(button);
 });
 it("preserves explanatory strings as text and follows RTL language",()=>{
  setLanguage("fa");const root=helpDisclosure('<img src=x onerror=alert(1)>');
  expect(root.querySelector("img")).toBeNull();expect(root.querySelector(".settings-help-content")!.textContent).toContain("<img");
  expect(root.querySelector(".settings-help-content")!.getAttribute("dir")).toBe("rtl");
  expect(root.querySelector("button")!.getAttribute("aria-label")).toBe("توضیح بیشتر");
 });
 it("shared titles and rows hide only explanations and identify their help",()=>{
  const status=document.createElement("span");status.setAttribute("role","status");status.textContent="Busy";
  const head=sectionHead({id:"sec-test",icon:"agents",title:"Agents",desc:"Static explanation",badge:status});
  const input=document.createElement("input");input.id="model";
  const row=settingRow({label:"Model",hint:"Choose one",forId:"model",extra:status.cloneNode(true)},input);
  expect(head.querySelector("h2")!.textContent).toBe("Agents");expect(head.querySelector("button")!.getAttribute("aria-label")).toContain("Agents");
  expect(row.querySelector("label")!.htmlFor).toBe("model");expect(row.querySelector("button")!.getAttribute("aria-label")).toContain("Model");
  expect((row.querySelector(".settings-help-content") as HTMLElement).hidden).toBe(true);
  expect(row.querySelector('[role="status"]')!.textContent).toBe("Busy");expect(input.hidden).toBe(false);
  for(const name of ["dashboard","arrowBack","help"] as const)expect(icon(name).querySelector("path")).not.toBeNull();
 });
});
