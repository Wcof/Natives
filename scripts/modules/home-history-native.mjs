// Actual Native → Runtime → isolated ledger; current assets without saved history.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
export async function verifyNativeHomeHistory(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 const out=resolve(import.meta.dirname,'../../.tmp/home-history/native');mkdirSync(out,{recursive:true});
 const posted=[];page.on('request',r=>{if(r.url().includes('/api/finance/snapshot/save')&&r.method()==='POST')posted.push(JSON.parse(r.postData()));});
 await runtime.evaluate(async()=>{
  const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/assets',{account:'Isolated history',name:'Isolated cash',productType:'cash',riskClass:'cash',marketValue:'123.45',valuedAt:today});
  await Fund.actions.refreshHomeValuation();Fund.home.applyDisplayMode('amount',false);
 });
 const initial=await runtime.evaluate(()=>Fund.home.snapshot());assert.equal(initial.portfolio.history.points.length,1);
 for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [1280,320]){
  await page.setViewportSize({width,height:900});await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
  await runtime.waitForFunction(({theme,language})=>document.documentElement.dataset.theme===theme&&Fund.state.language===(language==='en'?'en':'zh-CN'),{theme,language});
  assert.equal(await frame.locator('#home-history-chart circle').getAttribute('cx'),'300');
  assert.match(await frame.locator('#home-history-chart').innerText(),/123.45/);
  assert.equal(await frame.locator('#home-history-chart polyline').count(),0);
  await frame.locator('[data-history-save]').focus();await frame.locator('[data-history-save]').press('Enter');
  const check=frame.locator('dialog[open] [name=flowsChecked]');await check.waitFor();
  assert.equal(await check.isChecked(),false);assert.equal(await check.evaluate(el=>document.activeElement===el),true);
  await frame.locator('dialog[open]').press('Escape');
  assert.equal(await frame.locator('[data-history-save]').evaluate(el=>document.activeElement===el),true);assert.equal(posted.length,0);
  await frame.locator('#home-history-chart').scrollIntoViewIfNeeded();await page.screenshot({path:resolve(out,`${theme}-${language}-${width}.png`)});
  assert.equal(await runtime.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),true);
 }
 await frame.locator('[data-history-save]').click();
 const saved=page.waitForResponse(r=>r.url().includes('/api/finance/snapshot/save')&&r.request().method()==='POST');
 await frame.locator('dialog[open] [data-kind=snapshot] [type=submit]').click();
 const response=await saved;assert.equal(response.status(),200);assert.ok((await response.json()).id>0);assert.equal(posted[0].flowsChecked,false);
 await frame.locator('dialog[open]').press('Escape');
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));assert.equal(await frame.locator('#home-history-chart svg').count(),0);
 await frame.locator('[data-history-reveal]').click();await runtime.waitForFunction(()=>Fund.home.displayMode()==='amount');
 report.steps.push({name:'Current real assets, no historical snapshots: centered readable value; 8 theme/language/width combinations; record entry, unchecked verification, cancel/focus, explicit real snapshot save and privacy reveal',valueMinor:initial.wealth.totalA,posted,shots:out});
}
