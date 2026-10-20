// Production renderers with contract-shaped synthetic responses; Native coverage is separate.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
const ui=resolve(import.meta.dirname,'../../modules/fund/ui/src');
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try{
 const page=await browser.newPage();const errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.setContent(`<main class="main-column">${readFileSync(resolve(ui,'partials/view-home.html'),'utf8')}<section id="view-stock-detail"><div id="sd-holding-context"></div></section></main>`);
 for(const file of ['core.js','home-i18n.js'])await page.addScriptTag({path:resolve(ui,file)});
 await page.evaluate(()=>{
  window.mode='amount';window.calls=[];Fund.state.language='en';
  Fund.home={displayMode:()=>mode,privacy:()=>mode!=='amount',escape:s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])),money:value=>Fund.formatMinorExact(value),snapshot:()=>({wealth:{cash:{accounts:[]}}}),status(){},requestId:()=>crypto.randomUUID()};
  Fund.api=async(method,url)=>{calls.push(url);if(url.startsWith('/api/holding/records')){
   if(window.deferRecord)return new Promise(resolve=>{window.releaseRecord=resolve;});
   const page=Number(new URL(url,'http://fixture').searchParams.get('page'));
   return {page,hasMore:page===1,dataRevision:'r1',transactions:Array.from({length:page===1?20:1},(_,i)=>({id:page===1?21-i:1,tradeDate:'2026-01-07',type:'BUY',state:'confirmed',quantity:'1',price:'2.1',fee:'0.01'})),opening:null};
  }return {};};
 });
 for(const file of ['home-overview.js','home-manage.js','holding-summary.js'])await page.addScriptTag({path:resolve(ui,file)});
 await page.evaluate(()=>{document.getElementById('view-home').hidden=false;window.groups={configured:true,groups:[{groupId:'g1',name:'Equity',currentMinor:'10000',currentTenths:1000,ratioTenths:1000,assets:[]}],unassignedMinor:'0',unallocatedTenths:0};Fund.home.renderGroupAllocation(groups);});
 const summary=page.locator('[data-group-id="g1"] > summary');await summary.click();await summary.focus();
 await page.evaluate(()=>Fund.home.renderGroupAllocation({...groups,groups:[{...groups.groups[0],currentMinor:'11000'}]}));
 assert.equal(await page.locator('[data-group-id="g1"]').getAttribute('open'),'');
 assert.equal(await summary.evaluate(node=>document.activeElement===node),true);
 assert.match(await page.locator('.distribution-children').textContent(),/No assets assigned/);
 await page.evaluate(()=>{document.getElementById('view-home').hidden=true;Fund.holdingSummary.set({accountId:1,fundId:2,account:'Synthetic account',fundName:'Synthetic stock',marketValue:'10',quantity:'1',averageCost:'2',income:{holding:{amount:'8'},day:{amount:null,date:'2026-01-07'}}});});
 await page.locator('[data-holding-records]').click();await page.locator('#sd-holding-panel [data-record-dir=next]:not([disabled])').waitFor();
 assert.equal(await page.locator('#sd-holding-panel tbody tr').count(),20);
 assert.ok(!(await page.locator('#sd-holding-panel').innerText()).includes('Insufficient history'));
 await page.locator('[data-record-dir=next]').click();await page.waitForFunction(()=>document.querySelector('#sd-holding-panel nav span').textContent==='2');
 assert.equal(await page.locator('#sd-holding-panel tbody tr').count(),1);assert.equal(await page.locator('[data-record-dir=next]').isDisabled(),true);
 assert.equal(await page.locator('[data-record-dir=prev]').evaluate(node=>document.activeElement===node),true,'last page focus moves to an available navigation control');
 await page.evaluate(()=>{Fund.state.language='zh-CN';document.dispatchEvent(new Event('fund:language-changed'));});
 await page.locator('[data-record-dir=prev]').click();await page.waitForFunction(()=>document.querySelector('#sd-holding-panel nav span').textContent==='1');
 assert.equal(await page.locator('#sd-holding-panel tbody tr').count(),20);
 await page.locator('button[data-holding-tab=overview]').click();
 await page.evaluate(()=>{window.deferRecord=true;Fund.holdingSummary.set({accountId:1,fundId:2,account:'Synthetic account',fundName:'Synthetic stock'});});await page.locator('[data-holding-records]').click();
 await page.evaluate(()=>{Fund.holdingSummary.set({accountId:3,fundId:4,account:'New account'});releaseRecord({page:1,hasMore:false,transactions:[]});});
 await page.waitForTimeout(50);assert.equal(await page.locator('dialog[open]').count(),0,'old account response must not reopen a dialog');
 assert.deepEqual(errors,[]);console.log('group disclosure/focus, empty state, record paging and stale account: passed');
}finally{await browser.close();}
