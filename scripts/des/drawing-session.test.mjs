// Persistence-owner regression. API replies are synthetic; Rust integration checks DB atomicity.
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { resolve } from 'node:path';
import { mkdirSync, writeFileSync } from 'node:fs';
const root=resolve(import.meta.dirname,'../..'),out=resolve(root,'.tmp/des-drawing-session');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const report={scope:'synthetic-persistence-owner',steps:[],errors:[]};
try {
 const page=await browser.newPage();page.on('pageerror',e=>report.errors.push(e.message));
 await page.setContent('<div id="toolbar"></div>');
 for(const file of ['core.js','home-i18n.js','drawing-session.js'])await page.addScriptTag({path:resolve(root,'modules/fund/ui/dist',file)});
 await page.evaluate(()=>{
  window.scope={symbol:'600519',period:'day',viewContext:{fq:'qfq',seriesKind:'OHLC'}};window.rendered=[];window.calls=[];window.remote=[];window.failSave=false;window.failDelete=false;window.loseReceipt=false;
  window.mockApi=async(method,url,body)=>{
   calls.push({method,url,body});
   if(method==='GET')return {drawings:structuredClone(remote)};
   if(method==='DELETE'){if(failDelete)throw Object.assign(new Error('conflict'),{status:409});await new Promise(r=>window.deleteAck=r);remote=remote.filter(d=>!url.includes(d.id));return {success:true};}
   if(failSave)throw Object.assign(new Error('offline'),{status:500});
   const drawing={...body,digest:'b'.repeat(64)};remote=[drawing];if(loseReceipt){loseReceipt=false;throw new Error('lost acknowledgement');}return {success:true,drawing};
  };
  window.fetch=async(url,options={})=>{try{return new Response(JSON.stringify(await mockApi(options.method||'GET',url,options.body?JSON.parse(options.body):undefined)));}catch(e){if(!e.status)throw e;return new Response(JSON.stringify({error:'APP_TEST_ERROR'}),{status:e.status});}};
  window.session=Fund.createDrawingSession({scope:()=>scope,replace:rows=>rendered=rows,statusRoot:()=>document.getElementById('toolbar')});
  window.draft={id:'d1',symbol:'sh600519',period:'day',toolType:'trendline',points:[{date:'2026-09-01',price:10},{date:'2026-09-02',price:11}],options:{}};
 });
 await page.evaluate(async()=>{remote=[{...draft,options:{},digest:'a'.repeat(64)},{...draft,id:'hfq',options:{coordinateVersion:2,viewContext:{fq:'hfq',seriesKind:'OHLC'}}}];await session.load();});
 assert.equal(await page.evaluate(()=>rendered.length),0);assert.match(await page.locator('details').innerText(),/旧图形|legacy/);report.steps.push('legacy is read-only; other fq is not rendered');
 await page.evaluate(async()=>{remote=[];await session.load();await session.save(draft);});
 assert.equal(await page.evaluate(()=>rendered[0].digest),'b'.repeat(64));assert.equal(await page.evaluate(()=>calls.at(-1).body.options.color),undefined);report.steps.push('save requires real acknowledgement; default color is absent');
 await page.evaluate(async()=>{failSave=true;const edit={...rendered[0],points:[{date:'2026-09-01',price:20},{date:'2026-09-02',price:21}]};await session.save(edit);});
 assert.equal(await page.evaluate(()=>rendered[0].points[0].price),10);assert.equal(await page.evaluate(()=>calls.at(-1).body.expectedDigest),'b'.repeat(64));
 await page.evaluate(()=>failSave=false);await page.getByRole('button',{name:'重试保存'}).click();await page.waitForFunction(()=>rendered[0]?.points[0].price===20);report.steps.push('failed edit rolls back confirmed geometry, retains draft and retries with digest');
 await page.evaluate(async()=>{loseReceipt=true;await session.save({...rendered[0],points:[{date:'2026-09-01',price:30},{date:'2026-09-02',price:31}]});});assert.equal(await page.evaluate(()=>rendered[0].points[0].price),30);report.steps.push('lost receipt is reconciled by same-id read without creating a second drawing');
 await page.evaluate(()=>session.manage());
 await page.locator('.drawing-object-list button').click();
 await page.locator('dialog[open] [name=name]').fill('Support review');
 await page.locator('dialog[open] [name=locked]').check();
 await page.locator('dialog[open] [name=visible]').uncheck();
 await page.locator('dialog[open] [name=price0]').fill('32');
 await page.locator('dialog[open] [type=submit]').click();
 await page.waitForFunction(()=>rendered[0]?.points[0].price===32);
 assert.equal(await page.evaluate(()=>rendered[0].options.locked),true);
 assert.equal(await page.evaluate(()=>rendered[0].options.visible),false);
 assert.equal(await page.evaluate(()=>rendered[0].options.color),undefined);
 await page.keyboard.press('Escape');
 report.steps.push('native object properties save exact anchors, visibility, lock and theme-default styling');
 await page.evaluate(()=>{window.removing=session.remove('d1');});await page.waitForFunction(()=>typeof deleteAck==='function');assert.equal(await page.evaluate(()=>rendered.length),1);await page.evaluate(async()=>{deleteAck();await removing;});assert.equal(await page.evaluate(()=>rendered.length),0);report.steps.push('delete retains confirmed drawing until server acknowledgement');
 assert.deepEqual(report.errors,[]);
} finally {await browser.close();writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');}
console.log(`${report.steps.length} drawing persistence checks; ${report.errors.length} page errors`);
