// 合成 API 错误／延迟回归；真实账本与导航另由 investment-suite-native 验证。
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from 'playwright-core';
const root=resolve(import.meta.dirname,'../..'),ui=resolve(root,'modules/fund/ui/src');
const out=resolve(root,'.tmp/investment-ui-audit');mkdirSync(out,{recursive:true});
const read=path=>readFileSync(resolve(ui,path),'utf8');
const rs=readFileSync(resolve(root,'modules/fund/src/ui.rs'),'utf8').split('const INDEX_HTML: &str = concat!(')[1].split('\n);')[0];
const html=[...rs.matchAll(/include_str!\("([^"]+)"\)|("(?:\\.|[^"\\])*")/g)].map(m=>m[1]?read(m[1].replace('../ui/dist/','')):JSON.parse(m[2])).join('').replace('<script src="app.js"></script>','');
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const report={scope:'synthetic-errors-and-delays',checks:[],errors:[]};
try{
 const page=await browser.newPage({viewport:{width:1280,height:900}});
 page.on('pageerror',e=>report.errors.push(e.message));
 await page.setContent(html);
 await page.addScriptTag({path:resolve(ui,'core.js')});
 await page.addScriptTag({path:resolve(ui,'home-i18n.js')});
 await page.evaluate(()=>{
   const syntheticSnapshot={wealth:{cash:{accounts:[]}}};
   Fund.home={state:{displayMode:'amount',privacy:false,snapshot:syntheticSnapshot},
     displayMode:()=>Fund.home.state.displayMode,
     privacy:()=>Fund.home.state.displayMode!=='amount',
     snapshot:()=>Fund.home.state.snapshot,
     applyDisplayMode:(mode)=>{Fund.home.state.displayMode=mode;},
     escape:s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])),
     refresh:async()=>{},status:s=>{window.statusMessage=s;},requestId:()=> 'synthetic-'+(++window.requestSequence)};
   window.requestSequence=0;
   window.calls=[];window.reads=[];window.holdPrefix=null;window.failPrefix=null;window.failWrite=false;
   window.assets=[
     {id:1,version:1,name:'Private deposit',location:'Private bank',productType:'deposit',valueMinor:'123456789012345678',valuedAt:'2026-10-01',deleted:false},
     {id:2,version:1,name:'Private gold',location:'Private safe',productType:'gold',valueMinor:'1200',valuedAt:'2026-10-01',deleted:false}];
   window.transactions=[
     {id:1,account:'Private account',fundCode:'110022',quantity:'1.1234',price:'1.2345',amount:'1.39',fee:'0.00',tradeDate:'2026-10-01',type:'BUY',state:'pending',version:1},
     {id:2,account:'Private account',fundCode:'600519',quantity:'100',price:'100',amount:'10000.00',fee:'1.00',tradeDate:'2026-10-01',type:'BUY',state:'confirmed',version:1},
     {id:3,account:'Private account',fundCode:'600519',quantity:'20',price:'100',amount:'2000.00',fee:'0.00',tradeDate:'2026-10-01',type:'SELL',state:'confirmed',version:1,deleted:true}];
   Fund.api=async(method,url,body,options={})=>{
     calls.push({method,url,body});
     if(method==='POST'&&url==='/api/transactions'){if(failWrite)throw Error('Synthetic rejected write');return {id:9,created:true};}
     if(method==='GET'&&holdPrefix&&url.startsWith(holdPrefix))return await new Promise((resolve,reject)=>reads.push({url,signal:options.signal,resolve,reject}));
     if(method==='GET'&&failPrefix&&url.startsWith(failPrefix))throw Error('Synthetic read failure');
     if(url.startsWith('/api/holding/records'))return {opening:null,transactions:[],hasMore:false};
     if(url.startsWith('/api/offline/assets/history'))return [{valuedAt:'2026-10-01',valueMinor:'1200'}];
     if(url.startsWith('/api/offline/assets/exits'))return {exits:[]};
     if(url.startsWith('/api/offline/assets')){const type=new URL(url,'http://test').searchParams.get('productType');return {assets:assets.filter(a=>!type||a.productType===type)};}
     if(url.startsWith('/api/legacy-assets'))return {assets:[{id:3,productType:'gold',name:'Private legacy gold',marketValue:'13.00'}]};
     if(url.startsWith('/api/transactions'))return {transactions:transactions.filter(tx=>url.includes('includeDeleted')||!tx.deleted)};
     return {holdings:[],assets:[],navs:[]};
   };
 });
 for(const file of ['ledger.js','assets.js','home-manage.js','positions-edit.js'])await page.addScriptTag({path:resolve(ui,file)});
 await page.evaluate(()=>{
   Fund.applyLanguage('en');document.dispatchEvent(new Event('fund:privacy-changed'));
   document.querySelector('.workspace-body').classList.add('side-collapsed');
   window.showView=async id=>{
     Fund.actions.leaveAssets();Fund.actions.leaveTx();Fund.home.closePanel();
     document.querySelectorAll('.main-column > section').forEach(v=>v.hidden=v.id!=='view-'+id);
     if(id==='assets')await Fund.actions.loadAssets();
     if(id==='tx')await Fund.actions.loadTx();
   };
 });
 await page.evaluate(()=>showView('assets'));
 assert.equal(await page.locator('#offline-summary b').first().innerText(),'1,234,567,890,123,468.78','BigInt page summary remains exact');
 assert.match(await page.locator('#offline-cards').innerText(),/Active/);
 await page.locator('#offline-filter').selectOption('deposit');
 await page.waitForFunction(()=>document.querySelectorAll('#offline-cards .offline-asset-card').length===1);
 assert.equal(await page.locator('#offline-legacy').innerText(),'','gold legacy is excluded by deposit filter');
 await page.evaluate(()=>{failPrefix='/api/offline/assets';return Fund.actions.loadAssets();});
 assert.match(await page.locator('#offline-cards').innerText(),/Private deposit/,'same-filter trusted rows survive failure');
 assert.equal(await page.locator('#offline-retry').isVisible(),true);
 await page.evaluate(()=>{failPrefix=null;holdPrefix='/api/offline/assets';Fund.actions.loadAssets();});
 await page.waitForFunction(()=>reads.length===1);
 await page.locator('#offline-filter').selectOption('other');
 await page.waitForFunction(()=>reads.length===2);
 assert.equal(await page.evaluate(()=>reads[0].signal.aborted),true);
 await page.evaluate(()=>{reads[1].resolve({assets:[]});holdPrefix=null;});
 await page.waitForFunction(()=>document.querySelector('#asset-msg').textContent==='');
 await page.evaluate(()=>reads[0].resolve({assets:[{...assets[0],name:'STALE-RESULT'}]}));
 assert.doesNotMatch(await page.locator('#offline-cards').innerText(),/STALE-RESULT|Private deposit/);
 assert.match(await page.locator('#offline-cards').innerText(),/No .*assets|No .*investments/i);
 report.checks.push('assets: exact page scope, active status, legacy filter, error preserves rows, retry, old-response rejection, explicit empty');
 await page.locator('#offline-filter').selectOption('deposit');
 await page.locator('[data-offline=history]').click();
 await page.locator('dialog[open] [data-close]').click();
 await page.evaluate(()=>{holdPrefix='/api/offline/assets/history';});
 await page.locator('[data-offline=history]').click();
 await page.waitForFunction(()=>reads.length===3);
 await page.locator('dialog[open] [data-close]').click();
 await page.evaluate(()=>reads[2].resolve([{valuedAt:'2026-10-01',valueMinor:'999'}]));
 assert.equal(await page.locator('dialog[open]').count(),0,'closed history does not reopen after late reply');
 await page.evaluate(()=>{holdPrefix=null;return showView('tx');});
 assert.equal(await page.locator('#tx-entry').evaluate(el=>el.open),false,'entry form is collapsed initially');
 assert.match(await page.locator('#tx-table').innerText(),/1.1234|1.2345/);
 const requests=await page.evaluate(()=>calls.length);
 await page.evaluate(()=>{Fund.home.applyDisplayMode('hidden', false);document.dispatchEvent(new Event('fund:privacy-changed'));});
 assert.equal(await page.evaluate(()=>calls.length),requests,'privacy redraw does not refetch');
 assert.doesNotMatch(await page.locator('#tx-table').innerText(),/Private|110022|600519|1.1234|1.2345/);
 assert.equal(await page.locator('#tx-entry').isVisible(),false);
 await page.evaluate(()=>{Fund.home.applyDisplayMode('amount', false);document.dispatchEvent(new Event('fund:privacy-changed'));});
 await page.locator('#tx-state-filter').selectOption('pending');
 assert.equal(await page.locator('#tx-table tbody tr').count(),1);
 await page.evaluate(()=>{failPrefix='/api/transactions';return Fund.actions.loadTx();});
 assert.match(await page.locator('#tx-table').innerText(),/110022/);
 assert.equal(await page.locator('#tx-retry').isVisible(),true);
 await page.evaluate(()=>{failPrefix=null;holdPrefix='/api/transactions';Fund.actions.loadTx();});
 await page.waitForFunction(()=>reads.length===4);
 await page.locator('#tx-show-deleted').check();
 await page.waitForFunction(()=>reads.length===5);
 assert.equal(await page.evaluate(()=>reads[3].signal.aborted),true);
 await page.evaluate(()=>{reads[4].resolve({transactions:transactions});holdPrefix=null;});
 await page.waitForFunction(()=>document.querySelector('#tx-load-msg').textContent==='');
 await page.evaluate(()=>reads[3].resolve({transactions:[]}));
 assert.match(await page.locator('#tx-table').innerText(),/110022/);
 await page.locator('#tx-entry > summary').click();
 assert.equal(await page.locator('#tx-form [name=fee]').inputValue(),'0.00');
 assert.match(await page.locator('#tx-form [name=tradeDate]').inputValue(),/^\d{4}-\d{2}-\d{2}$/);
 assert.equal(await page.locator('#tx-form [name=state]').inputValue(),'pending');
 await page.locator('#tx-form [name=instrumentKind]').selectOption('stock');
 assert.equal(await page.locator('#tx-form [name=quantity]').getAttribute('step'),'1');
 assert.equal(await page.locator('#tx-state-row').isVisible(),false);
 await page.locator('#tx-form [name=instrumentKind]').selectOption('fund');
 assert.equal(await page.locator('#tx-form [name=quantity]').getAttribute('step'),'0.0001');
 await page.locator('#tx-form [name=fundCode]').fill('110022');
 await page.locator('#tx-form [name=quantity]').fill('1.1234');
 await page.locator('#tx-form [name=price]').fill('1.2345');
 await page.evaluate(()=>{failWrite=true;});
 await page.locator('#tx-form [type=submit]').click();
 await page.locator('#tx-msg.err').waitFor();
 assert.equal(await page.locator('#tx-form [name=quantity]').inputValue(),'1.1234');
 await page.locator('#tx-form [type=submit]').click();
 await page.waitForFunction(()=>calls.filter(c=>c.method==='POST'&&c.url==='/api/transactions').length===2);
 const keys=await page.evaluate(()=>calls.filter(c=>c.method==='POST'&&c.url==='/api/transactions').map(c=>c.body.requestId));
 assert.equal(keys[0],keys[1],'identical failed submission reuses idempotency key');
 await page.evaluate(()=>{Fund.applyLanguage('zh-CN');});
 assert.equal(await page.locator('#tx-form [name=quantity]').inputValue(),'1.1234');
 await page.evaluate(()=>{holdPrefix='/api/transactions';Fund.actions.loadTx();Fund.actions.leaveTx();document.getElementById('view-tx').hidden=true;});
 assert.equal(await page.evaluate(()=>reads.at(-1).signal.aborted),true);
 report.checks.push('transactions: collapse, default date/fee, integer stocks/4-digit funds/pending, failure draft and key preserved, local filter, privacy no network, read race and leave cancellation');
 await page.evaluate(()=>{
   holdPrefix=null;failPrefix='/api/holding/records';
   window.testHolding={accountId:1,fundId:2,account:'Private account',fundCode:'110022',quantity:'10',cost:'8'};
   Fund.actions.openHoldingEditor(testHolding);
 });
 await page.locator('[data-record-retry]').waitFor();
 assert.match(await page.locator('dialog[open]').innerText(),/Synthetic read failure/);
 await page.evaluate(()=>{failPrefix=null;});await page.locator('[data-record-retry]').click();
 await page.waitForFunction(()=>document.querySelector('[data-record-next]'));
 assert.equal(await page.locator('[data-record-next]').isDisabled(),true);
 assert.equal(await page.locator('[data-record-prev]').isDisabled(),true);
 await page.keyboard.press('Escape');
 const pendingRead=await page.evaluate(()=>{
   holdPrefix='/api/holding/records';const n=reads.length;Fund.actions.openHoldingEditor(testHolding);return n;
 });
 await page.waitForFunction(n=>reads.length>n,pendingRead);
 await page.keyboard.press('Escape');
 await page.waitForFunction(n=>reads[n].signal.aborted,pendingRead);
 await page.evaluate(n=>reads[n].resolve({opening:null,transactions:[],hasMore:false}),pendingRead);
 assert.equal(await page.locator('dialog[open]').count(),0,'late holding records cannot reopen a dismissed editor');
 const privacyRead=await page.evaluate(()=>{const n=reads.length;Fund.actions.openHoldingEditor(testHolding);return n;});
 await page.waitForFunction(n=>reads.length>n,privacyRead);
 await page.evaluate(()=>{Fund.home.applyDisplayMode('hidden', false);document.dispatchEvent(new Event('fund:privacy-changed'));});
 await page.waitForFunction(n=>reads[n].signal.aborted,privacyRead);
 assert.equal(await page.locator('dialog[open]').count(),0);
 await page.evaluate(n=>{reads[n].resolve({opening:null,transactions:[],hasMore:false});holdPrefix=null;Fund.home.applyDisplayMode('amount', false);},privacyRead);
 report.checks.push('holding editor: explicit empty, bounded pagination, read failure/retry, Escape cancels, late reply rejected, privacy closes and aborts');
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [320,640,1280]){
   await page.setViewportSize({width,height:900});
   await page.evaluate(({theme,language})=>{holdPrefix=null;Fund.applyLanguage(language);document.documentElement.dataset.theme=theme;},{theme,language});
   for(const view of ['assets','tx']){
     await page.evaluate(view=>showView(view),view);
     const metrics=await page.evaluate(()=>({width:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth,main:document.querySelector('.main-column').clientWidth,mainScroll:document.querySelector('.main-column').scrollWidth}));
     assert.ok(metrics.scroll<=metrics.width+1,JSON.stringify({theme,language,width,view,metrics}));
     assert.ok(metrics.mainScroll<=metrics.main+1,JSON.stringify({theme,language,width,view,metrics}));
     if(view==='assets')assert.equal(await page.locator('#offline-cards').evaluate(el=>el.scrollWidth<=el.clientWidth+1),true,'asset cards remain within their grid');
     else assert.ok(await page.locator('#view-'+view+' .home-table-scroll').evaluate(el=>el.scrollWidth>el.clientWidth||el.clientWidth>900),'tables scroll locally or have adequate width');
     await page.screenshot({path:resolve(out,view+'-'+theme+'-'+language+'-'+width+'.png')});
   }
 }
 assert.deepEqual(report.errors,[]);
 report.checks.push('assets/transactions: Archive + Volt, Chinese + English, 320/640/1280, local table scroll');
 console.log(report.checks.join('\n'));
}finally{writeFileSync(resolve(out,'synthetic-report.json'),JSON.stringify(report,null,2)+'\n');await browser.close();}
