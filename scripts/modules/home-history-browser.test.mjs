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
  for(const file of ['core.js','home-i18n.js','home-trend-chart.js','home-overview.js','home.js','home-range.js','home-manage.js'])await page.addScriptTag({path:resolve(root,file)});
  await page.evaluate(async theme=>{document.documentElement.dataset.theme=theme;Fund.state.token='test';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);},theme);
  for(const language of ['zh-CN','en']){
   await page.evaluate(language=>Fund.applyLanguage(language),language);
   // Nearby amounts must remain distinguishable on the axis, including fractional ticks.
   for(const values of [[298000,300000,302000],[299975,300000,300025],[300000,300000.5,300001],[298000000,300000000,302000000],[0,0,0]]){
    await page.evaluate(values=>{
     const s=structuredClone(Fund.home.snapshot());
     s.groupAllocation.groups.forEach(group=>{group.assets=[];});
     s.portfolio.history.todayPoints=[];
     s.portfolio.history.timeResolution='day';
     s.portfolio.history.points=values.map((value,i)=>({date:new Date(Date.parse(s.requestedRange.today+'T00:00:00Z')-(values.length-1-i)*86400000).toISOString().slice(0,10),valueMinor:String(Math.round(value*100)),quality:'ready'}));
     Fund.home.renderOverview(s);
    },values);
    const labels=await page.locator('#home-history-chart .history-y span').allTextContents();
    assert.ok(labels.length>=3,'real amount ticks are present');
    assert.equal(new Set(labels).size,labels.length,`distinct ticks must not collapse to one amount: ${labels}`);
    if(values[0]===298000){
     assert.deepEqual(labels,language==='en'?['302K','300K','298K']:['30.2万','30万','29.8万']);
     await page.locator('#home-history-chart').screenshot({path:resolve(out,`axis-${theme}-${width}-${language}.png`)});
    }
    if(language==='en')assert.ok(labels.every(label=>!/[万亿]/.test(label)),'axis units follow the surface language');
    const plot=page.locator('#home-history-chart .history-plot');
    await plot.focus();await plot.press('End');
    assert.match(await page.locator('#home-history-chart .history-tip').innerText(),new RegExp(values.at(-1).toLocaleString('en-US',{minimumFractionDigits:2,maximumFractionDigits:2}).replace(/[.*+?^${}()|[\]\\]/g,'\\$&')),'cursor preserves the underlying amount');
    await plot.press('Escape');
    assert.equal(await page.locator('#home-history-chart .history-tip').isHidden(),true);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true,'precise ticks fit narrow layouts');
   }
   // A year is a query window, not an instruction to pad missing history with zeros.
   await page.evaluate(()=>{
    const s=structuredClone(Fund.home.snapshot());
    const day=n=>new Date(Date.parse(s.requestedRange.today+'T00:00:00Z')-n*86400000).toISOString().slice(0,10);
    s.requestedRange={...s.requestedRange,key:'year1',from:day(365)};
    s.portfolio.history.todayPoints=[];
    s.portfolio.history.points=[
     {date:day(365),valueMinor:'0',quality:'incomplete'},
     {date:day(30),valueMinor:'10000',quality:'ready'},
     {date:day(29),valueMinor:'11000',quality:'ready'},
     {date:day(28),valueMinor:null,quality:'incomplete'},
     {date:day(1),valueMinor:'0',quality:'ready'},
     {date:day(0),valueMinor:null,quality:'incomplete'},
    ];
    Fund.home.renderOverview(s);
   });
   const coverageLine=page.locator('#home-history-chart [data-series="total"] path[fill="none"]');
   assert.match(await coverageLine.getAttribute('d'),/^M2,/,'leading missing dates must not occupy the axis');
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').getAttribute('cx'),'598','trailing missing dates must not occupy the axis');
   const coverageLabels=await page.evaluate(()=>{
    const today=Fund.home.snapshot().requestedRange.today;
    return [30,1].map(n=>new Date(Date.parse(today+'T00:00:00+08:00')-n*86400000).toLocaleDateString(Fund.state.language,{timeZone:'Asia/Shanghai'}));
   });
   assert.deepEqual(await page.locator('#home-history-chart .history-axis span').allTextContents(),coverageLabels,'axis labels match the actual covered month');
   const coveragePlot=page.locator('#home-history-chart .history-plot');
   assert.ok((await coveragePlot.boundingBox()).height>=260,'trend has enough vertical space');
   const bounds=await coveragePlot.boundingBox();
   await page.mouse.move(bounds.x+bounds.width*.9,bounds.y+bounds.height*.5);
   assert.match(await page.locator('.history-tip').innerText(),/0\.00/,'pointer follows actual time, including a true zero after a gap');
   assert.equal(await coverageLine.count(),1,'missing days do not connect to the zero');
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`coverage-${theme}-${width}-${language}.png`)});
   await page.evaluate(()=>{
    const s=structuredClone(Fund.home.snapshot());s.portfolio.history.points=[{date:s.requestedRange.today,valueMinor:'12345678',quality:'reference'}];
    Fund.home.renderOverview(s);
   });
   assert.equal(await page.locator('#home-history-chart circle').getAttribute('cx'),'300','single real point is centered and visible');
   assert.match(await page.locator('#home-history-chart').textContent(),/123,456.78/,'single value is readable without opening details');
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),0,'one observation never invents a trend');
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
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),0,'current assets cannot backfill a historical range');
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
   // 分类卡只在「有可绘制走势」时出现：没有走势的分类不出卡（点下去只会得到空白图）。
   await page.evaluate(()=>{
    const s=structuredClone(Fund.home.snapshot());
    s.groupAllocation.groups[0].currentMinor='41723';
    s.portfolio.history.points=[];
    Fund.home.renderOverview(s);
   });
   assert.equal(await page.locator('#home-history-categories [data-history-cat="g1"]').count(),0,'categories without a drawable trend are not offered');
   await page.evaluate(()=>{
    const s=structuredClone(Fund.home.snapshot());
    const today=s.requestedRange.today,back=n=>new Date(Date.parse(today+'T00:00:00Z')-n*86400000).toISOString().slice(0,10);
    s.groupAllocation.groups[0].currentMinor='41723';
    s.portfolio.history.points=[
      {date:back(2),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'10000'}]},
      {date:back(1),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
    ];
    Fund.home.renderOverview(s);
   });
   assert.equal(await page.locator('#home-history-categories [data-history-cat="g1"]').count(),1,'categories with a real trend are offered');
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
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
   // 分类缺历史时保持空态，不回退到全资产总额。
   assert.equal(await page.locator('.history-empty').count(),1,'missing category history remains empty');
   // 夹具里的分类没有绑定资产：状态行必须说明「为什么这一类画不出来」，
   // 而不是让图上凭空少一条线（有资产但估值不足两天时用另一条文案）。
   assert.match(await page.locator('#home-history-status').innerText(),/至少两个日期|at least two dates|没有可回放|no replayable|no trend to draw/);
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`category-${theme}-${width}-${language}.png`)});
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),0,'one current observation is not a fabricated trend');
   await category({currentMinor:'0'});
   assert.doesNotMatch(await page.locator('#home-history-chart').innerText(),/123,456/,'zero category does not show total assets');
   await category({currentMinor:null});
   assert.doesNotMatch(await page.locator('#home-history-chart').innerText(),/123,456/,'unknown category does not show total assets');
   const today=await page.evaluate(()=>Fund.home.snapshot().requestedRange.today);
   const ago=days=>new Date(Date.parse(today+'T00:00:00Z')-days*86400000).toISOString().slice(0,10);
   const at=Date.parse(today+'T09:30:00+08:00')/1000;
   const observation=(offset,value)=>({date:today,observedAt:at+offset,valueMinor:'12345678',quality:'reference',groups:[{groupId:'g1',valueMinor:value}]});
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'10000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),0,'category view excludes the total anchor');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),1,'tail anchor marks the category value');
   assert.equal(await page.locator('#home-history-chart [data-series="total"] path[fill="none"]').count(),0,'category view excludes the total line');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'two real intraday observations must draw a trend');
   const spread=await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').evaluate(node=>{const b=node.getBBox();return {w:b.width,h:b.height};});
   assert.ok(spread.w>0,'same-day timestamps are distinct x coordinates');
   assert.ok(spread.h>0,'each observation keeps its own category amount');
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'20000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'unchanged real observations draw a flat line');
   await category({currentMinor:'41723',intraday:true,points:[observation(0,'10000'),observation(3600,'20000')]});
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),0,'hidden or disconnected periods are not filled');
   const dailyObserved=[2,1,0].map((n,i)=>({date:ago(n),observedAt:Date.parse(ago(n)+['T20:10:00+08:00','T22:45:00+08:00','T11:30:00+08:00'][i])/1000,valueMinor:String(30000+i*1000),quality:'reference',source:'valuation-observation',groups:[{groupId:'g1',valueMinor:String(10000+i*1000)}]}));
   await category({currentMinor:'41723',points:dailyObserved});
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'daily category view excludes total assets');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'daily categories use the same calendar mapping');
   await category({currentMinor:'41723',points:[dailyObserved[0],dailyObserved[2]]});
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),0,'a genuinely missing calendar day still breaks the curves');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),2,'isolated valid points must not disappear');
   await category({currentMinor:'99999999',points:dailyObserved.map((point,i)=>({...point,observedAt:undefined,source:i===2?undefined:point.source,groups:[{groupId:'g1',valueMinor:i===2?null:'10000'}]}))});
   assert.ok((await page.locator('#home-history-chart .history-y span').allTextContents()).every(label=>Number(label.replaceAll(',',''))<200),'explicit unknown group values cannot be replaced by a live amount');
   await category({currentMinor:'41723',points:dailyObserved.slice(0,2).map(p=>({...p,groups:[]})),todayPoints:[observation(0,'10000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'intraday category fallback excludes the daily total');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'mixed-resolution charts retain real minute observations');
   await category({currentMinor:'41723',points:[{date:ago(1),valueMinor:'12345678',quality:'ready',groups:[]}],todayPoints:[observation(0,'10000'),observation(60,'20000')]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'new categories automatically use their real intraday observations while older total history remains daily');
   await category({currentMinor:'41723',points:[{date:today,valueMinor:'12345678',quality:'reference',groups:[]}]});
   assert.doesNotMatch(await page.locator('#home-history-chart').innerText(),/123,456/,'a live total cannot fill missing category history');
   await category({currentMinor:'41723',past:ago(1),points:[
    {date:ago(2),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'10000'}]},
    {date:ago(1),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
   ]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),0,'category view excludes the total anchor');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),1,'tail anchor marks the category value');
   assert.equal(await page.locator('#home-history-chart g[data-series] path[fill="none"]').count(),1,'only the selected category sets the scale');
   assert.match(await page.locator('#home-history-categories [data-history-cat="g1"]').innerText(),/\+100\.00%/,'category card reports its own range change');
   const replay=[3,2,1,0].map((n,i)=>({date:ago(n),valueMinor:String(30000+i*1000),quality:'reference',source:'holding-nav-replay',groups:[{groupId:'g1',valueMinor:String(10000+i*1000),basis:'current-bindings'}]}));
   await category({currentMinor:'41723',points:replay});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] circle').count(),0,'category view excludes the total anchor');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),1,'tail anchor marks the category value');
   assert.equal(await page.locator('#home-history-chart [data-series="total"] path[fill="none"]').count(),0);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'historical NAV reference points must connect into a real curve');
   assert.match(await page.locator('#home-history-categories [data-history-cat="g1"]').innerText(),/\+30\.00%/,'replay category change uses its own NAV values');
   assert.match(await page.locator('#home-history-selection').innerText(),/Equity Group/,'selecting one type enters its detail level');
   assert.match(await page.locator('#home-history-status').textContent(),/当前归属|current assignments/);
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`reconstructed-${theme}-${width}-${language}.png`)});
   replay[1].groups[0].valueMinor=null;
   await category({currentMinor:'41723',points:replay});
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'missing category NAV does not restore the total');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),1,'missing NAV keeps drawable segments connected');
   const gapPath=await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').evaluate(n=>n.getAttribute('d'));
   assert.match(gapPath,/^M[\d.,-]+L[\d.,-]+$/,'a missing price keeps only the surviving two-point segment');
   await category({currentMinor:'41723',past:ago(1),points:[
    {date:ago(3),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'10000'}]},
    {date:ago(2),valueMinor:'12345678',quality:'ready',groups:[]},
    {date:ago(1),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
   ]});
   assert.equal(await page.locator('#home-history-chart [data-series="total"] path[fill="none"]').count(),0);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] circle').count(),2,'both isolated observations stay visible across missing dates');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"] path[fill="none"]').count(),0,'missing category observations break the category curve');
   await category({currentMinor:'41723',past:ago(1),points:[{date:ago(1),valueMinor:'12345678',quality:'ready',groups:[]}]});
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),0,'current category amounts never backfill old records');
   assert.equal(await page.locator('.history-empty').count(),1,'a lone total cannot fill missing historical category values');
   // 全部资产含总额与分类；选中某个分类只画它下面的资产。
   await page.locator('#home-history-categories [data-history-cat="total"]').click();
   await page.evaluate(today=>{
     const original=Fund.home.snapshot;
     window.__originalSnapshot=original;
     const s=structuredClone(original());
     const days=Array.from({length:4},(_,i)=>new Date(Date.parse(today+'T00:00:00Z')-(3-i)*86400000).toISOString().slice(0,10));
     s.groupAllocation.groups[0].assets=[{historyId:'fund-1-1',name:'Fund A',code:'110022',kind:'fund',accountId:1},{historyId:'fund-1-2',name:'Fund B',code:'110023',kind:'fund',accountId:1}];
     s.groupAllocation.groups.push({groupId:'g3',name:'Other Group',ratioTenths:0,currentTenths:0,currentMinor:'10000',assets:[{historyId:'fund-1-3',name:'Fund C',code:'110024',kind:'fund',accountId:1}]});
     s.portfolio.history.points=days.map((date,i)=>({date,valueMinor:String(30000000+i*1000),quality:'reference',source:'valuation-observation',groups:[{groupId:'g1',valueMinor:String(60000+i*600),assets:[{historyId:'fund-1-1',valueMinor:String(40000+i*400)},{historyId:'fund-1-2',valueMinor:String(20000+i*200)}]},{groupId:'g3',valueMinor:String(10000+i*100),assets:[{historyId:'fund-1-3',valueMinor:String(10000+i*100)}]}]}));
     s.requestedRange={...s.requestedRange,from:days[0],to:days.at(-1),today:days.at(-1)};
     window.__trendSnapshot=s;
     Fund.home.snapshot=()=>window.__trendSnapshot;
     Fund.home.renderOverview(s);
   },today);
   assert.equal(await page.locator('#home-history-categories [data-history-cat]').count(),3,'top row lists 全部资产 + each category with a drawable trend');
   assert.equal(await page.locator('#home-holding-cards [data-history-cat]').count(),3,'bottom row lists every asset while all assets are selected');
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),1);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),1,'all assets selected draws category lines');
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
   assert.equal(await page.locator('#home-holding-cards [data-history-cat]').count(),2,'selecting a category narrows the asset row to it');
   assert.match(await page.locator('#home-holding-cards strong').innerText(),/Equity Group · (具体持仓|Holdings)/);
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),0,'detail level replaces the category aggregate');
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-1"]').count(),1);
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-2"]').count(),1);
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-1"] path[fill="none"]').count(),1,'selected holding retains its actual connected curve');
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-2"] path[fill="none"]').count(),1);
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'the 300,000 total is excluded from category details');
   assert.ok((await page.locator('#home-history-chart .history-y span').allTextContents()).every(label=>Number(label.replaceAll(',',''))<1000),'the amount axis scales to the selected holdings only');
   await page.locator('#home-history-chart .history-plot').press('End');
   const detailTip=await page.locator('#home-history-chart .history-tip').innerText();
   assert.match(detailTip,/412\.00/);assert.match(detailTip,/206\.00/);assert.doesNotMatch(detailTip,/300,030|总资产|Total assets/);
   await page.locator('#home-history-chart').screenshot({path:resolve(out,`selected-${theme}-${width}-${language}.png`)});
   for(const view of ['percent','amount']){
    await page.locator(`[data-history-view="${view}"]`).click();
    assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'view switches preserve category-only scope');
   }
   for(const source of ['groups','holdingValues']){
    await page.evaluate(source=>{
     const s=structuredClone(window.__trendSnapshot);
     s.groupAllocation.groups[0].assets[0].valueMinor='99999999';
     const last=s.portfolio.history.points.at(-1);
     delete last.source;
     if(source==='groups')last.groups[0].assets[0].valueMinor=null;
     else {last.groups=[];last.holdingValues={'fund-1-1':null};}
     Fund.home.renderOverview(s);
    },source);
    assert.ok((await page.locator('#home-history-chart .history-y span').allTextContents()).every(label=>Number(label.replaceAll(',',''))<1000),'saved unknown holding values cannot be overwritten by current holdings');
    await page.locator('#home-history-chart .history-plot').press('End');
    assert.match(await page.locator('.history-tip').innerText(),/缺少|No valuation|missing|No data/i,'tooltip exposes the missing holding value');
   }
   await page.evaluate(()=>{const s=structuredClone(window.__trendSnapshot);s.portfolio.history.points.forEach(p=>p.groups.forEach(g=>g.assets?.forEach(a=>{a.valueMinor=null;})));Fund.home.renderOverview(s);});
   assert.equal(await page.locator('.history-single').count(),0,'missing holding values never fall back to the live total');
   await page.evaluate(()=>Fund.home.renderOverview(window.__trendSnapshot));
   await page.locator('#home-holding-cards [data-history-cat="fund-1-1"]').click();
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-1"]').count(),0,'hidden asset is not drawn');
   assert.equal(await page.locator('#home-holding-cards [data-history-cat="fund-1-1"]').getAttribute('aria-pressed'),'false');
   const secondHolding=page.locator('#home-holding-cards [data-history-cat="fund-1-2"]');
   if(await secondHolding.isHidden())await page.locator('#home-holding-cards .home-history-cat-fold').click();
   await secondHolding.click();
   assert.equal(await page.locator('#home-history-chart [data-series]').count(),0,'hiding all holdings never restores the total');
   const detailFold=page.locator('#home-holding-cards .home-history-cat-fold');
   if(await detailFold.isVisible()&&await detailFold.getAttribute('aria-expanded')==='true')await detailFold.click();
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
   assert.equal(await page.locator('#home-holding-cards [data-history-cat="fund-1-1"]').getAttribute('aria-pressed'),'true','picking the selected category again restores every asset');
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),1,'picking it again returns to the category level');
   await page.locator('#home-history-categories [data-history-cat="g3"]').click();
   assert.equal(await page.locator('#home-holding-cards [data-history-cat]').count(),1,'the asset row follows the selected category');
   assert.equal(await page.locator('#home-history-chart [data-series="fund-1-3"]').count(),1,'assets of the selected category share the chart');
   assert.match(await page.locator('#home-history-selection').innerText(),/Other Group/);
   await page.locator('#home-history-categories [data-history-cat="total"]').click();
   assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),1,'全部资产 restores the category level');
   assert.equal(await page.locator('#home-holding-cards [data-history-cat]').count(),3,'全部资产 lists every asset again');
   await page.evaluate(()=>{window.__multiTrendSnapshot=window.__trendSnapshot;window.__trendSnapshot=structuredClone(window.__trendSnapshot);window.__trendSnapshot.groupAllocation.groups=window.__trendSnapshot.groupAllocation.groups.slice(0,1);Fund.home.renderOverview(window.__trendSnapshot);});
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),1,'All assets retains total even with only one category');
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'a sole category remains separately selectable');
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),1,'clicking the sole category again restores All assets');
   await page.locator('#home-history-categories [data-history-cat="g1"]').click();
   await page.evaluate(()=>{const s=structuredClone(window.__trendSnapshot);s.groupAllocation.groups=[];Fund.home.renderOverview(s);});
   assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),1,'removing the selected category restores the overall chart');
   await page.evaluate(()=>{window.__trendSnapshot=window.__multiTrendSnapshot;Fund.home.renderOverview(window.__trendSnapshot);});
   // 卡行超过两行折叠：只留前两行，提示里带剩余数量；点开后恢复完整列表。
   await page.evaluate(()=>{
     const s=structuredClone(window.__trendSnapshot);
     s.groupAllocation.groups[0].assets=Array.from({length:12},(_,i)=>({historyId:'fund-x-'+i,name:'Asset '+i,code:'9000'+i,kind:'fund',accountId:1}));
     window.__trendSnapshot=s;
     Fund.home.renderOverview(s);
   });
   const fold=page.locator('#home-holding-cards .home-history-cat-fold');
   assert.equal(await fold.isHidden(),false,'more than two rows offers a fold control');
   assert.match(await fold.innerText(),/还有 \d+ 项|\d+ more/);
   const folded=await page.locator('#home-holding-cards .history-cat-card:not([hidden])').count();
   const total=await page.locator('#home-holding-cards .history-cat-card').count();
   assert.ok(total>=12,'every asset still has a card');
   assert.ok(folded<total,'collapsed view shows only the first row');
   await fold.click();
   assert.equal(await page.locator('#home-holding-cards .history-cat-card:not([hidden])').count(),total,'expanding reveals every card');
   assert.match(await fold.innerText(),/收起|Collapse/);
   await fold.click();
   assert.equal(await page.locator('#home-holding-cards .history-cat-card:not([hidden])').count(),folded,'collapsing again hides the overflow');
   // 周期按钮：本周/半年/一年 使用新的区间关键字并真正进入快照请求。
   for (const range of ['week','half','year1']) {
     const before=await page.evaluate(()=>requests.length);
     await page.evaluate(value=>Fund.actions.setHomeRange(value),range);
     assert.ok((await page.evaluate(n=>requests.slice(n),before)).some(r=>r.path.includes(`range=${range}`)),`${range} range reaches the snapshot request`);
   }
   await page.evaluate(()=>{Fund.home.snapshot=window.__originalSnapshot;});
   await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
   assert.doesNotMatch(await page.locator('#home-history-status').innerText(),/Equity Group/,'hidden status masks selected category identity');
   await page.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
   await page.locator('#home-history-categories [data-history-cat="total"]').click();
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
