export interface WakeWord { addressed:boolean; command:string }

/** Match only an explicit name at the beginning; preserve the original command. */
export function parseWakeWord(text:string):WakeWord{
 let normalized="";const ends:number[]=[];let offset=0;
 for(const character of text){offset+=character.length;if(/[\u064b-\u065f\u0670\u06d6-\u06ed\u0640]/u.test(character))continue;
  const value=character.replace(/[يى]/u,"ی").replace(/ك/u,"ک").replace(/\u200c/u," ").toLowerCase();
  for(const part of value){normalized+=part;ends.push(offset);}
 }
 // The pinned Persian recognizer also transcribes the name with ب; keep exact boundaries.
 const match=/^\s*(?:(?:سلام|hello|hi)[\s،,:!؟?؛;.-]+)?(?:رودیپ|رو\s+دیپ|رودیب|رو\s+دیب|roadeep)(?=$|[\s،,:!؟?؛;.-])[\s،,:!؟?؛;.-]*/u.exec(normalized);
 return match?{addressed:true,command:match[0].length===normalized.length?"":text.slice(ends[match[0].length-1]??0).trim()}:{addressed:false,command:text};
}
