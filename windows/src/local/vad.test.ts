import {describe,it,expect} from "vitest";
import {UtteranceDetector} from "./vad";
describe("bounded continuous utterances",()=>{
 it("keeps silence idle without dispatching",()=>{const vad=new UtteranceDetector(16000);for(let i=0;i<10000;i++)expect(vad.push(new Float32Array(1600))).toBeUndefined();});
 it("rejects short noises and segments speech after silence",()=>{const vad=new UtteranceDetector(16000);expect(vad.push(new Float32Array(1600).fill(.1))).toBeUndefined();expect(vad.push(new Float32Array(16000))).toBeUndefined();const speech=vad.push(new Float32Array(32000).fill(.05));expect(speech).toBeUndefined();expect(vad.push(new Float32Array(16000))?.length).toBeLessThanOrEqual(16000*4);});
 it("retains a short name plus real captured quiet tail until speaker verification can process it",()=>{
  const vad=new UtteranceDetector(16000);const chunks=[new Float32Array(1600),new Float32Array(4800).fill(.05),...Array.from({length:17},()=>new Float32Array(1600))];let actual:Float32Array|undefined;let consumed=0;
  for(const chunk of chunks){actual=vad.push(chunk);consumed+=chunk.length;if(actual)break;}
  expect(actual?.length).toBeGreaterThanOrEqual(32000);expect(actual?.length).toBe(consumed);expect(Array.from(actual!.subarray(1600,6400))).toEqual(Array.from(chunks[1]));expect(actual!.subarray(6400).every(value=>value===0)).toBe(true);
 });
 it("keeps a two hundred millisecond noise rejected even with a long tail and handles empty callbacks",()=>{const vad=new UtteranceDetector(16000);expect(vad.push(new Float32Array())).toBeUndefined();expect(vad.push(new Float32Array(3200).fill(.05))).toBeUndefined();for(let i=0;i<30;i++)expect(vad.push(new Float32Array(1600))).toBeUndefined();});
 it("bounds nonstop speech to less than 30 seconds and resets",()=>{const vad=new UtteranceDetector(16000);let result:Float32Array|undefined;for(let i=0;i<300&&!result;i++)result=vad.push(new Float32Array(1600).fill(.05));expect(result?.length).toBeLessThanOrEqual(16000*30);expect(result?.length).toBeGreaterThan(16000*28);expect(vad.push(new Float32Array(1600))).toBeUndefined();});
});
