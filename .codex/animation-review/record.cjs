const { chromium }=require('C:/Users/Amir/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const fs=require('fs');const path=require('path');
(async()=>{
 const browser=await chromium.launch({channel:'msedge',headless:true});
 const page=await browser.newPage({viewport:{width:1000,height:720}});
 await page.goto('http://127.0.0.1:1420/dev/character-preview.html');
 await page.waitForFunction(()=>!!window.characterPreview);
 const bytes=await page.evaluate(async()=>{
  await document.fonts.ready;
  const sheet=document.createElement('canvas');sheet.width=1000;sheet.height=600;
  const ctx=sheet.getContext('2d');
  const cards=[...document.querySelectorAll('.motion-card')];
  const stream=sheet.captureStream(30);
  const type=MediaRecorder.isTypeSupported('video/webm;codecs=vp9')?'video/webm;codecs=vp9':'video/webm';
  const recorder=new MediaRecorder(stream,{mimeType:type,videoBitsPerSecond:3000000});
  const chunks=[];recorder.ondataavailable=e=>chunks.push(e.data);
  const done=new Promise(resolve=>recorder.onstop=resolve);
  window.characterPreview.replay();
  const started=Date.now();
  function draw(){
   ctx.fillStyle='#0e0f11';ctx.fillRect(0,0,1000,600);
   ctx.fillStyle='#f5f6f8';ctx.font='600 24px Vazirmatn';ctx.textAlign='center';ctx.fillText('حرکت‌های تازهٔ کاراکتر Roadeep',500,45);
   for(let i=0;i<cards.length;i++){
    const c=cards[i],x=(3-i%4)*250,y=70+Math.floor(i/4)*255;
    ctx.drawImage(c.querySelector('canvas'),x+45,y+15,160,160);
    ctx.fillStyle='#f5f6f8';ctx.font='17px Vazirmatn';ctx.fillText(c.querySelector('strong').textContent,x+125,y+208);
   }
   if(Date.now()-started<4200)requestAnimationFrame(draw);else recorder.stop();
  }
  recorder.start();draw();await done;stream.getTracks().forEach(t=>t.stop());
  return Array.from(new Uint8Array(await new Blob(chunks,{type:'video/webm'}).arrayBuffer()));
 });
 fs.writeFileSync(path.join(__dirname,'character-motions.webm'),Buffer.from(bytes));
 await browser.close();console.log('Saved character-motions.webm ('+bytes.length+' bytes)');
})().catch(e=>{console.error(e);process.exit(1)});
