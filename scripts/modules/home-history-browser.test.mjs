// Real Chrome renderer, synthetic read-only snapshots; no user ledger.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {readFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {installHomeFixture} from './home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../../modules/fund/ui/src'),out=resolve(import.meta.dirname,'../../.tmp/home-history');
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try {
 for(const theme of ['archive','volt'])for(const width of [1280,320]){
  const page=await browser.newPage({viewport:{width,height:900}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(`<style>${readFileSync(resolve(root,'../dist/styles/theme.css'),'utf8')}${readFileSync(resolve(root,'styles/home.css'),'utf8')}${readFileSync(resolve(root,'styles/investment.css'),'utf8')}</style>${readFileSync(resolve(root,'partials/view-home.html'),'utf8')}`);
  await page.evaluate(installHomeFixture);
  for(const file of ['core.js','home-i18n.js','home-overview.js','home.js','home-range.js','home-manage.js'])await page.addScriptTag({path:resolve(root,file)});
  await page.evaluate(async theme=>{document.documentElement.dataset.theme=theme;Fund.state.token='test';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);},theme);
  for(const language of ['zh-CN','en']){
   await page.evaluate(language=>Fund.applyLanguage(language),language);
   await page.evaluate(()=>{
    const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[{date:s.requestedRange.today,valueMinor:'12345678',quality:'reference'}];
    Fund.home.renderOverview(s);
   });
   assert.equal(await page.locator('#home-history-chart circle').getAttribute('cx'),'300','single real point is centered and visible');
   assert.match(await page.locator('#home-history-chart').textContent(),/123,456.78/,'single value is readable without opening details');
   assert.equal(await page.locator('#home-history-chart polyline').count(),0,'one observation never invents a trend');
   await page.evaluate(()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[];s.wealth.totalA=null;s.wealth.knownA='12345';s.wealth.quality='incomplete';Fund.home.renderOverview(s);});
   assert.match(await page.locator('#home-history-chart').textContent(),/123.45/,'known partial assets remain visible');
   assert.equal(await page.locator('#home-history-chart [data-history-save]').count(),1);
   await page.evaluate(()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[{date:s.requestedRange.today,valueMinor:'10',quality:'incomplete'},{date:'2026-09-01',valueMinor:'20',quality:'incomplete'}];Fund.home.renderOverview(s);});
   assert.match(await page.locator('#home-history-chart').textContent(),/123,456.78/,'unusable history does not suppress the actual current valuation');
   await page.evaluate(()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[{date:s.requestedRange.today,valueMinor:'0',quality:'ready'}];Fund.home.renderOverview(s);});
   assert.match(await page.locator('#home-history-chart').textContent(),/0.00/,'real zero is not treated as missing');
   await page.evaluate(()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[];s.wealth.totalA=null;s.wealth.knownA='12345';s.wealth.quality='incomplete';Fund.home.renderOverview(s);});
   await page.screenshot({path:resolve(out,`${theme}-${width}-${language}.png`),fullPage:true});
   await page.evaluate(()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[];s.requestedRange.to='2026-08-01';Fund.home.renderOverview(s);});
   assert.equal(await page.locator('#home-history-chart circle').count(),0,'current assets cannot backfill a historical range');
   assert.equal(await page.locator('#home-history-chart [data-history-today]').count(),1);
   await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
   assert.equal(await page.locator('#home-history-chart svg').count(),0);assert.doesNotMatch(await page.locator('#home-history-chart').textContent(),/123,456|123.45/);
   const reveal=page.locator('[data-history-reveal]');await reveal.focus();await reveal.press('Enter');
   assert.equal(await page.evaluate(()=>Fund.home.displayMode()),'amount');
   const before=await page.evaluate(()=>requests.filter(r=>r.path.includes('/snapshot/save')).length);
   await page.evaluate(async()=>{const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[];window.snapshotOverride=s;await Fund.actions.setHomeRange('today');});
   await page.locator('[data-history-save]').focus();await page.locator('[data-history-save]').press('Enter');
   await page.locator('dialog[open] [data-kind=snapshot]').waitFor();
   assert.equal(await page.locator('dialog[open] [name=flowsChecked]').evaluate(el=>el===document.activeElement),true);
   await page.locator('dialog[open]').press('Escape');
   assert.equal(await page.locator('[data-history-save]').evaluate(el=>el===document.activeElement),true);
   assert.equal(await page.evaluate(()=>requests.filter(r=>r.path.includes('/snapshot/save')).length),before,'opening/cancelling never records an estimate');
   await page.locator('[data-history-group=g1]').click();
   const category=async data=>page.evaluate(data=>{
    const snapshot=structuredClone(Fund.home.snapshot());
    snapshot.groupAllocation.groups[0].currentMinor=data.currentMinor;
    snapshot.portfolio.history.points=data.points||[];
    snapshot.portfolio.history.timeResolution=data.intraday?'minute':'day';
    snapshot.portfolio.history.todayPoints=data.todayPoints||[];
    if(data.past)snapshot.requestedRange.to=data.past;
    Fund.home.renderOverview(snapshot);
   },data);
   await category({currentMinor:'41723'});
   // Demo 口径：总资产常显，分类只是叠加线；分类没有历史不能顶替总额。
   assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','without category history the total remains the headline');
   assert.match(await page.locator('#home-history-status').innerText(),/至少两个日期|at least two dates/);
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`category-${theme}-${width}-${language}.png`)});
   assert.equal(await page.locator('#home-history-chart polyline').count(),0,'one current observation is not a fabricated trend');
   await category({currentMinor:'0'});
   assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','a zero category never replaces the total headline');
   await category({currentMinor:null});
   assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','unknown category keeps the total current value');
   const today=await page.evaluate(()=>Fund.home.snapshot().requestedRange.today);
   const ago=days=>new Date(Date.parse(today+'T00:00:00Z')-days*86400000).toISOString().slice(0,10);
   const at=Date.parse(today+'T09:30:00+08:00')/1000;
   const observation=(offset,value)=>({date:today,observedAt:at+offset,valueMinor:'12345678',quality:'reference',groups:[{groupId:'g1',valueMinor:value}]});
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'10000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),2);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),2);
   assert.equal(await page.locator('#home-history-chart [data-series="total"] polyline').count(),1,'total assets stay visible while a category is selected');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] polyline').count(),1,'two real intraday observations must draw a trend');
   const positions=await page.locator('#home-history-chart [data-series="g1"] circle').evaluateAll(nodes=>nodes.map(node=>({x:node.getAttribute('cx'),y:node.getAttribute('cy')})));
   assert.notEqual(positions[0].x,positions[1].x,'same-day timestamps are distinct x coordinates');
   assert.notEqual(positions[0].y,positions[1].y,'each observation keeps its own category amount');
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'20000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] polyline').count(),1,'unchanged real observations draw a flat line');
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'10000'),observation(3600,'20000')]});
   assert.equal(await page.locator('#home-history-chart polyline').count(),0,'hidden or disconnected periods are not filled');
   await category({currentMinor:'41723',points:[{date:ago(1),valueMinor:'12345678',quality:'ready',groups:[]}],todayPoints:[observation(0,'10000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] polyline').count(),1,'new categories automatically use their real intraday observations while older total history remains daily');
   await category({currentMinor:'41723',points:[{date:today,valueMinor:'12345678',quality:'reference',groups:[]}]});
   assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','a live total reference is not replaced by the category amount');
   await category({currentMinor:'41723',past:ago(1),points:[
    {date:ago(2),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'10000'}]},
    {date:ago(1),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
   ]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),2);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),2);
   assert.equal(await page.locator('#home-history-chart polyline').count(),2,'total and the selected category share one normalized scale');
   assert.match(await page.locator('[data-history-group="g1"]').innerText(),/\+100\.00%/,'category tab reports its own range change');
   const replay=[3,2,1,0].map((n,i)=>({date:ago(n),valueMinor:String(30000+i*1000),quality:'reference',source:'holding-nav-replay',groups:[{groupId:'g1',valueMinor:String(10000+i*1000),basis:'current-bindings'}]}));
   await category({currentMinor:'41723',points:replay});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),4);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),4);
   assert.equal(await page.locator('#home-history-chart [data-series="total"] polyline').count(),1);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] polyline').count(),1,'historical NAV reference points must connect into a real curve');
   assert.match(await page.locator('[data-history-group="g1"]').innerText(),/\+30\.00%/,'replay category change uses its own NAV values');
   assert.match(await page.locator('#home-history-selection').innerText(),/总资产 \+ Equity Group|Total assets \+ Equity Group/);
   assert.match(await page.locator('#home-history-status').textContent(),/当前归属|current assignments/);
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`reconstructed-${theme}-${width}-${language}.png`)});
   replay[1].groups[0].valueMinor=null;
   await category({currentMinor:'41723',points:replay});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),4,'missing NAV does not remove total points');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),3,'missing NAV is not a zero');
   const replayLines=await page.locator('#home-history-chart [data-series="g1"] polyline').evaluateAll(ns=>ns.map(n=>n.getAttribute('points').split(' ').length));
   assert.deepEqual(replayLines,[2],'a missing price splits historical reference lines');
   await category({currentMinor:'41723',past:ago(1),points:[
    {date:ago(3),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'10000'}]},
    {date:ago(2),valueMinor:'12345678',quality:'ready',groups:[]},
    {date:ago(1),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
   ]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),3);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),2);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] polyline').count(),0,'missing category observations break the category curve');
   await category({currentMinor:'41723',past:ago(1),points:[{date:ago(1),valueMinor:'12345678',quality:'ready',groups:[]}]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),0,'current category amounts never backfill old records');
   assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','a lone historical total remains visible');
   await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
   assert.doesNotMatch(await page.locator('#home-history-status').innerText(),/Equity Group/,'hidden status masks selected category identity');
   await page.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
   await page.locator('#home-history-series [data-history-reset]').click();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  }
  const callsBefore=await page.evaluate(()=>requests.length);
  await page.evaluate(()=>Fund.actions.refreshHomeValuation());
  assert.ok((await page.evaluate(n=>requests.slice(n),callsBefore)).some(r=>r.method==='POST'&&r.path.startsWith('/api/home/valuation/observe?')),'visible refresh uses the explicit observation writer');
  const readBefore=await page.evaluate(()=>requests.length);
  await page.evaluate(()=>Fund.home.readSnapshot());
  assert.ok((await page.evaluate(n=>requests.slice(n),readBefore)).some(r=>r.method==='GET'&&r.path.startsWith('/api/home/snapshot?')),'readSnapshot never observes');
  const confirmed=await page.evaluate(()=>JSON.stringify(Fund.home.snapshot()));
  await page.evaluate(async()=>{window.failRange=true;await Fund.actions.refreshHomeValuation();window.failRange=false;});
  assert.equal(await page.evaluate(()=>JSON.stringify(Fund.home.snapshot())),confirmed,'failed observation keeps the confirmed snapshot');
  assert.match(await page.locator('#home-range-status').innerText(),/失败|failed/i);
  await page.evaluate(()=>{window.holdHomeReads=true;void Fund.actions.refreshHomeValuation();});
  await page.waitForFunction(()=>heldHomeRequests.some(r=>r.path.startsWith('/api/home/valuation/observe')));
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
  assert.equal(await page.evaluate(()=>heldHomeRequests.find(r=>r.path.startsWith('/api/home/valuation/observe')).signal.aborted),true,'hiding cancels an in-flight observation');
  const hiddenCount=await page.evaluate(()=>requests.length);
  await page.evaluate(()=>Fund.actions.refreshHomeValuation());
  assert.equal(await page.evaluate(()=>requests.length),hiddenCount,'hidden pages cannot start observations');
  await page.evaluate(()=>{heldHomeRequests.forEach(r=>r.resolve());window.holdHomeReads=false;Fund.actions.leaveHome();});
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('Home history: intraday trend/flat line/gap/category values, read-only queries, hidden cancellation, privacy, valuation entry/cancel and 8 theme/language/width layouts passed');
}finally{await browser.close();}
