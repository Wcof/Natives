import assert from 'node:assert/strict';
import {readFileSync, mkdirSync} from 'node:fs';
import {createServer} from 'node:http';
import {resolve, extname} from 'node:path';
import {chromium} from 'playwright-core';

const root=resolve(import.meta.dirname,'..'),ext=resolve(root,'extension');
const out=resolve(root,'.tmp/space-background');mkdirSync(out,{recursive:true});
const html=readFileSync(resolve(ext,'space.html'),'utf8').replace(/<script\b[^>]*>[\s\S]*?<\/script>/g,'');
const svg='<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480"><rect width="640" height="480" fill="#bb4433"/><path d="M0 0L640 480" stroke="#ffd090" stroke-width="80"/></svg>';
const server=createServer((req,res)=>{
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end(html);return;}
  if(req.url==='/wallpaper.svg'){res.setHeader('Content-Type','image/svg+xml');res.end(svg);return;}
  try {const file=resolve(ext,'.'+new URL(req.url,'http://localhost').pathname);if(!file.startsWith(ext+'/'))throw Error();res.setHeader('Content-Type',({'.js':'text/javascript','.css':'text/css','.svg':'image/svg+xml'})[extname(file)]||'application/octet-stream');res.end(readFileSync(file));}
  catch {res.statusCode=404;res.end();}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try {
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(base);
 await page.evaluate(async base=>{
   const {createSpaceDashboard}=await import('/space-dashboard.js');
   const colour=(await import('/plugins/backgrounds/colour.js')).colourBackground;
   const online=(await import('/plugins/backgrounds/online.js')).onlineBackground;
   window.dashboard=createSpaceDashboard({$:id=>document.getElementById(id),t:(_,f)=>f,selectedLanguage:'zh_CN',backgroundPlugins:{'background/colour':colour,'background/online':online},widgetPlugins:{},nativeCall:()=>{throw Error('layout test must not call Native');},broadcastRevision:()=>{},queueWorkspaceMutation:()=>{},toast:()=>{}});
   window.paint=key=>dashboard.render({backgroundJson:{key,display:key==='background/online'?{url:base+'/wallpaper.svg'}:{colour:'#bb4433'}},widgets:[]},'layout-test',()=>{});
 },base);
 for(const theme of ['archive','volt'])for(const width of [320,640,1280])for(const collapsed of [false,true])for(const key of ['background/colour','background/online']) {
   await page.setViewportSize({width,height:800});
   await page.evaluate(({theme,collapsed,key})=>{document.documentElement.dataset.theme=theme;document.body.classList.toggle('sidebar-collapsed',collapsed);paint(key);},{theme,collapsed,key});
   const geometry=await page.evaluate(()=>{
     const host=document.getElementById('dashboard-host'),bg=host.shadowRoot.querySelector('.background-layer'),shell=document.querySelector('.space-shell');
     const box=el=>{const r=el.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};};
     return {host:box(host),bg:box(bg),shell:box(shell),height:innerHeight,width:innerWidth};
   });
   assert.equal(geometry.bg.y,0,'background must start at the top: toolbar must not reserve a black row');
   assert.equal(geometry.bg.height,geometry.height);assert.equal(geometry.bg.x,geometry.shell.x);assert.equal(geometry.bg.width,geometry.shell.width);
   if(collapsed){assert.equal(geometry.bg.x,0);assert.equal(geometry.bg.width,geometry.width);}
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
 }
 await page.evaluate(()=>{
   const root=document.getElementById('dashboard-host').shadowRoot;
   const card=document.createElement('div');card.className='Widget';card.style.cssText='height:1200px;width:min(288px,calc(100vw - 32px));';
   const button=document.createElement('button');button.id='slot-refresh-probe';button.textContent='Refresh';button.style.cssText='float:right;width:36px;height:32px';
   window.slotRefreshes=0;button.onclick=()=>slotRefreshes++;card.append(button);root.querySelector('.middleCentre').append(card);
   const free=document.createElement('div');free.id='free-position-probe';free.className='Slot free free-slot-wrap';free.style.cssText='left:0;top:0;width:20px;height:20px';root.querySelector('.container').append(free);
 });
 for(const width of [320,640,1280]){
   await page.setViewportSize({width,height:800});
   const target=page.locator('#dashboard-host #slot-refresh-probe');
   const geometry=await target.evaluate(button=>{const b=button.getBoundingClientRect(),host=document.getElementById('dashboard-host'),toolbar=document.getElementById('dashboard-toolbar').getBoundingClientRect();return {top:b.top,toolbarBottom:toolbar.bottom,freeTop:host.shadowRoot.getElementById('free-position-probe').getBoundingClientRect().top,hit:document.elementFromPoint(b.x+b.width/2,b.y+b.height/2)===host&&host.shadowRoot.elementFromPoint(b.x+b.width/2,b.y+b.height/2)===button};});
   assert.ok(geometry.top>geometry.toolbarBottom,'automatic slot clears the floating toolbar');assert.equal(geometry.hit,true);assert.equal(geometry.freeTop,0,'free placement is preserved');
   const before=await page.evaluate(()=>slotRefreshes);await target.click();await target.press('Enter');assert.equal(await page.evaluate(()=>slotRefreshes),before+2);
 }
 await page.setViewportSize({width:1280,height:800});await page.screenshot({path:resolve(out,'background-full.png')});
 await page.locator('#space-settings-btn').focus();await page.locator('#space-settings-btn').press('Enter');
 assert.equal(await page.locator('#space-settings-btn').evaluate(el=>document.elementFromPoint(el.getBoundingClientRect().x+15,el.getBoundingClientRect().y+15)===el||el.contains(document.elementFromPoint(el.getBoundingClientRect().x+15,el.getBoundingClientRect().y+15))),true,'floating toolbar remains hittable');
 await page.evaluate(()=>dashboard.render({backgroundJson:{key:'background/colour',display:{colour:'#ffffff'}},widgets:[]},'layout-test',()=>{}));
 const contrast=await page.evaluate(()=>{
   const bg=getComputedStyle(document.getElementById('dashboard-toolbar')).backgroundColor.match(/[\d.]+/g).map(Number);
   const button=getComputedStyle(document.getElementById('space-settings-btn')),fg=button.color.match(/[\d.]+/g).map(Number),opacity=Number(button.opacity);
   const surface=bg.slice(0,3).map(n=>n*bg[3]+255*(1-bg[3]));
   const ink=fg.slice(0,3).map((n,i)=>n*opacity+surface[i]*(1-opacity));
   const luminance=v=>v.map(n=>{const s=n/255;return s<=.04045?s/12.92:((s+.055)/1.055)**2.4;}).reduce((a,n,i)=>a+n*[.2126,.7152,.0722][i],0);
   return (luminance(ink)+.05)/(luminance(surface)+.05);
 });
 assert.ok(contrast>=3,'floating toolbar icons stay legible even over a white wallpaper');
 assert.deepEqual(errors,[]);console.log('Space real Chrome layout: 24 background/theme/width/sidebar combinations cover full height; floating toolbar remains keyboard reachable');
} finally {await browser.close();await new Promise(r=>server.close(r));}
