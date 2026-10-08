/** Bounds idle audio to a short pre-roll; processing never queues another utterance. */
export class UtteranceDetector {
 private chunks:Float32Array[]=[];private length=0;private voiced=0;private quiet=0;private started=false;
 constructor(private rate:number){}
 reset(){this.chunks=[];this.length=this.voiced=this.quiet=0;this.started=false;}
 push(input:Float32Array):Float32Array|undefined{
  if(!input.length)return;
  let power=0;for(const value of input)power+=value*value;
  const speech=Math.sqrt(power/input.length)>.012;
  if(speech){this.started=true;this.voiced+=input.length;this.quiet=0;}else this.quiet+=input.length;
  this.chunks.push(input.slice());this.length+=input.length;
  if(!this.started){while(this.length>this.rate*.3&&this.chunks.length>1)this.length-=this.chunks.shift()!.length;return;}
  const accepted=this.voiced>=this.rate*.25;
  // Speaker verification needs two seconds of real PCM. Keep the actual quiet
  // tail of a short name instead of throwing it away or manufacturing samples.
  if(this.length<this.rate*29&&(speech||this.quiet<this.rate*.8||(accepted&&this.length<this.rate*2)))return;
  const output=accepted?new Float32Array(this.length):undefined;
  if(output){let offset=0;for(const chunk of this.chunks){output.set(chunk,offset);offset+=chunk.length;}}
  this.reset();return output;
 }
}
