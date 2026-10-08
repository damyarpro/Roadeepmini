import {describe,it,expect} from "vitest";
import {islandSize,VOICE_COMPACT_H,HEADER_EXTRA_H,VIEW_LAYOUTS} from "../core/layout";
import {dockedSize} from "../island/dock";
describe("visible microphone geometry",()=>{
 it("reserves the 44 px stop target while collapsed on every dock",()=>{const active=islandSize("compact","overview",0,null,true);expect(active.h).toBe(VOICE_COMPACT_H);expect(active.h).toBeGreaterThanOrEqual(44);expect(dockedSize("left","compact",active).w).toBeGreaterThanOrEqual(44);expect(islandSize("compact","overview").h).toBe(32);});
 it("keeps the fixed home content area after expanding the header",()=>{expect(islandSize("expanded","overview").h).toBe(VIEW_LAYOUTS.overview.height+HEADER_EXTRA_H);});
});
