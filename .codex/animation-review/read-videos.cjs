const { chromium } = require('C:/Users/Amir/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const fs = require('fs');
const path = require('path');
const files = ['7bo7VnGuFiLJg1qSzR26WYtgMY','Z8HrH1ARtmcnwOjVD150MhQFH0','DgNQdh4vakQ1VVxxiU7LuCMxU','oVt9XNhpLGWpn0nWmZdmbs48Ds','xyg0LhysDsmw8IAJKAlCBttl94','k0Nvlmi8z3nVam9zHhIPeBSfOQ','mcOfozhe2cqst0p5MOJt4Xyz6I','1Y2uizPEWNa2PQFD8vHZwueBpe0','zoArDAj8l41CXhnrfdSQtq0Rfo4','KRmZWgesMzNebWTHpmWj5z0UOc','nK46moW6bJe6qkunLsSMnlCxrs','mdSkPTbuZSKvUNxGp8MjCFf0g','ZRoxuZJ1zZ6m6tfd0aiIsORX2I'];
(async () => {
 const browser = await chromium.launch({channel:'msedge',headless:true});
 const page = await browser.newPage({viewport:{width:1000,height:660}});
 await page.goto('http://127.0.0.1:1431');
 const metadata=[];
 for (const [index, name] of files.entries()) {
  const info = await page.evaluate(async ({name,index}) => {
   document.body.replaceChildren(); document.body.style.cssText='margin:0;background:#202126';
   const blob=await (await fetch('/'+name+'.webm')).blob();
   const video=document.createElement('video'); video.muted=true;video.src=URL.createObjectURL(blob);
   document.body.append(video);
   await new Promise((res,rej)=>{video.onloadedmetadata=res;video.onerror=()=>rej(new Error('Video decode failed '+name));});
   const duration=video.duration;
   const canvas=document.createElement('canvas');canvas.width=1000;canvas.height=660;const ctx=canvas.getContext('2d');
   ctx.fillStyle='#202126';ctx.fillRect(0,0,1000,660);
   ctx.fillStyle='#fff';ctx.font='17px sans-serif';ctx.fillText((index+1)+'. '+name+' — '+duration.toFixed(2)+'s ('+video.videoWidth+'×'+video.videoHeight+')',15,24);
   for(let i=0;i<12;i++) {
    video.currentTime=Math.min(duration-0.025,0.01+(duration-0.04)*i/11);
    await new Promise((res,rej)=>{video.onseeked=res;video.onerror=rej;});
    const x=(i%4)*250,y=40+Math.floor(i/4)*205;
    ctx.fillStyle='#33353b';ctx.fillRect(x+4,y,242,178);
    const scale=Math.min(236/video.videoWidth,174/video.videoHeight);
    ctx.drawImage(video,x+125-video.videoWidth*scale/2,y+89-video.videoHeight*scale/2,video.videoWidth*scale,video.videoHeight*scale);
    ctx.fillStyle='#fff';ctx.font='13px sans-serif';ctx.fillText(video.currentTime.toFixed(2)+'s',x+10,y+194);
   }
   video.remove();document.body.append(canvas);
   return {duration,width:video.videoWidth,height:video.videoHeight};
  },{name,index});
  await page.screenshot({path:path.join(__dirname,`reference-${index+1}.png`)});
  metadata.push({index:index+1,name,...info}); console.log(index+1,name,info.duration);
 }
 fs.writeFileSync(path.join(__dirname,'video-metadata.json'),JSON.stringify(metadata,null,2));
 await browser.close();
})().catch(e=>{console.error(e);process.exit(1)});
