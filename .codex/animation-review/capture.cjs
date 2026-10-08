const { chromium } = require('C:/Users/Amir/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const fs = require('fs');
const path = require('path');
const tag=process.argv[2]||'iter-1';
(async()=>{
 const browser=await chromium.launch({channel:'msedge',headless:true});
 const page=await browser.newPage({viewport:{width:1440,height:900},colorScheme:'dark'});
 const errors=[];page.on('pageerror',e=>errors.push(String(e)));
 await page.goto('http://127.0.0.1:1420/dev/character-preview.html?animation=bounce');
 await page.waitForFunction(()=>!!window.characterPreview);
 await page.evaluate(()=>document.fonts.ready);
 for(const width of [375,768,1440]) {
  await page.setViewportSize({width,height:900});
  await page.evaluate(()=>window.characterPreview.renderAt(390));
  await page.screenshot({path:path.join(__dirname,`${tag}-${width}.png`),fullPage:true});
 }
 const names=['curious','nod','shake','bounce','spin','stretch','peek','sway'];
 const durations=[1500,900,760,1000,1250,1400,1300,1600];
 const frames=[];
 for(let i=0;i<names.length;i++) {
  for(const fraction of [0,.18,.38,.6,.82,1.1]) {
   await page.evaluate(({name,ms})=>{window.characterPreview.setAnimation(name);window.characterPreview.renderAt(ms);},{name:names[i],ms:durations[i]*fraction});
   const url=await page.locator('#hero canvas').evaluate(c=>c.toDataURL());
   frames.push({name:names[i],ms:Math.round(durations[i]*fraction),url});
  }
 }
 await page.evaluate(async frames=>{
  const sheet=document.createElement('canvas');sheet.width=1200;sheet.height=8*235;
  const ctx=sheet.getContext('2d');ctx.fillStyle='#141518';ctx.fillRect(0,0,sheet.width,sheet.height);
  for(let i=0;i<frames.length;i++) {
   const f=frames[i],x=i%6*200,y=Math.floor(i/6)*235;
   const img=new Image();img.src=f.url;await img.decode();ctx.drawImage(img,x,y,200,200);
   ctx.fillStyle='#f5f6f8';ctx.font='13px sans-serif';ctx.fillText(f.name+' '+f.ms+'ms',x+10,y+217);
  }
  window.reviewSheet=sheet.toDataURL();
 },frames);
 const sheet=await page.evaluate(()=>window.reviewSheet);
 fs.writeFileSync(path.join(__dirname,`${tag}-motion-sheet.png`),Buffer.from(sheet.split(',')[1],'base64'));
 await page.locator('#reduced').check();
 await page.evaluate(()=>window.characterPreview.renderAt(390));
 await page.screenshot({path:path.join(__dirname,`${tag}-reduced.png`),fullPage:true});
 await page.emulateMedia({colorScheme:'light'});
 await page.screenshot({path:path.join(__dirname,`${tag}-light.png`),fullPage:true});
 fs.writeFileSync(path.join(__dirname,`${tag}-errors.json`),JSON.stringify(errors));
 console.log(JSON.stringify({tag,errors,captures:5}));
 await browser.close();
})().catch(e=>{console.error(e);process.exit(1)});
