// Contract-shaped isolated Chrome regression; production Native ledger coverage is separate.
import assert from 'node:assert/strict';
import {readFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
const root=resolve(import.meta.dirname,'../..'),ui=resolve(root,'modules/fund/ui/src'),out=resolve(root,'.tmp/investment-final-fixes');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try{
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent('<div class="main-column"><section id="view-positions"><button id="source">Fund</button></section>'+readFileSync(resolve(ui,'partials/view-fund-detail.html'),'utf8')+'</div>');
 await page.addScriptTag({path:resolve(ui,'core.js')});
 await page.evaluate(()=>{
  Date.now=()=>Date.parse('2026-01-07T09:00:00Z');
  window.mode='amount';window.calls=[];window.plot=[];
  Object.assign(window.Fund,{state:{language:"en"},actions:{},home:{displayMode:()=>mode,snapshot:()=>({dataRevision:'r1',wealth:{totalA:'100000'}}),escape:s=>String(s??''),money:m=>(Number(m)/100).toFixed(2)},t:s=>s,themeColor:()=> '#345780',ensureEcharts:async()=>{}});
  window.echarts={getInstanceByDom:()=>null,init:el=>({setOption:option=>plot.push({id:el.id,option}),clear(){},dispose(){},resize(){}})};
  Fund.api=async(method,url,body,options)=>{calls.push({method,url,body});
   if(window.failQuarterCode&&url.startsWith('/api/fund/quarterly-positions?code='+failQuarterCode))return new Promise((resolve,reject)=>{window.lateQuarterError=reject;});
   if(window.pauseNavCode&&url.startsWith('/api/nav?code='+pauseNavCode))return new Promise(resolve=>{window.lateNav=resolve;});
   if(window.mixedNav&&url.startsWith('/api/nav?'))return {nav:[{navDate:'2025-01-07',unitNav:'2',source:'eastmoney-mobile'},{navDate:'2026-01-06',unitNav:'2.1',source:'eastmoney-mobile'},{navDate:'2026-01-07',unitNav:'1.3',source:'user_confirmed'}],nextCursor:null};
   if(url==='/api/positions/detail')return {holdings:[window.holding],accounts:[{id:1,name:'Synthetic account'}]};
   if(url==='/api/home/snapshot')return {dataRevision:'r1',wealth:{totalA:'100000'}};
   if(url.startsWith('/api/nav?'))return {nav:[{navDate:'2025-12-31',unitNav:'10'},{navDate:'2026-01-05',unitNav:'11'},{navDate:'2026-01-06',unitNav:'12'},{navDate:'2026-01-07',unitNav:'13'}],nextCursor:null};
   if(url.startsWith('/api/holding/income-history?'))return {dataRevision:'r1',from:'2026-01-01',to:'2026-01-07',rows:[{date:'2026-01-05',amount:'9.9',rateBps:'638'},{date:'2026-01-06',amount:'14.9',rateBps:'833'},{date:'2026-01-07',amount:'12',rateBps:'833'}],status:'ready',missingCount:0};
   if(url.startsWith('/api/fund/quarterly'))return window.quarterFixture||{fund_code:'110022',report_date:'2025-12-31',source:'fixture',stocks:[]};
   if(url.startsWith('/api/market/quotes?'))return {quotes:new URL(url,'http://fixture.test').searchParams.get('codes').split(',').map(code=>({code,changePct:1.25,asOf:'2026-01-07 15:00'}))};return {};};
  window.holding={accountId:1,fundId:2,fundCode:'110022',fundName:'Synthetic Fund',account:'Synthetic account',instrumentKind:'fund',venue:'otc',marketValue:'156',quantity:'12',cost:'124.08',averageCost:'10.34',dayPct:8.33,dayPnl:'12',holdingDays:7,asOf:'2026-01-07',dataRevision:'r1',income:{day:{amount:'12',date:'2026-01-07',rateBps:'833'},yesterday:{amount:'14.9',date:'2026-01-06',rateBps:'833'},holding:{amount:'31.92',rateBps:'2572'}}};
 });
 await page.addScriptTag({path:resolve(ui,'fund-detail-chart.js')});await page.addScriptTag({path:resolve(ui,'fund-detail.js')});await page.evaluate(()=>document.dispatchEvent(new Event('DOMContentLoaded')));
 await page.evaluate(()=>{window.confirmedHolding=structuredClone(holding);holding={...holding,marketValue:'144',nav:{nav:'12',date:'2026-01-06'},estimate:{status:'estimated',date:'2026-01-07',unitNav:'12.6',rateBps:'500',coverageBps:5000,reportDate:'2025-12-31',asOf:'2026-01-07 15:00'},income:{...holding.income,day:{amount:null,date:'2026-01-07'},estimatedDay:{amount:'7.2',marketValue:'151.2',date:'2026-01-07',estimated:true,rateBps:'500',coverageBps:5000}}};Fund.openFundDetail(holding);});
 assert.equal(await page.locator('#fd-val-day-income').textContent(),'+7.20');
 assert.equal(await page.locator('#fd-head-day-pct').textContent(),'+5.00%');
 assert.equal(await page.locator('#fd-estimate-note').isVisible(),true);
 await page.evaluate(()=>{holding=confirmedHolding;Fund.openFundDetail(holding);});
 assert.equal(await page.locator('#fd-val-day-income').textContent(),'+12.00');
 assert.equal(await page.locator('#fd-estimate-note').isVisible(),false,'confirmation takes precedence');
 await page.locator('#fd-tab-profit').click();await page.waitForTimeout(100);
 const rows=await page.locator('#fd-profit-tbody tr').allTextContents();assert.ok(rows.some(r=>r.includes('2026-01-05')&&r.includes('9.90')),'historical day must use ledger income 9.90 rather than current 12 shares × NAV difference');
 assert.deepEqual(await page.evaluate(()=>plot.find(p=>p.id==='fd-profit-chart').option.series[0].data),[9.9,14.9,12]);
 assert.equal(await page.locator('#fd-val-ratio').textContent(),'15.60%','managed total is minor units, independent of its size');
 assert.ok(await page.evaluate(()=>calls.some(c=>c.url.startsWith('/api/holding/income-history?'))),'private historical income must be queried by account and holding');
 assert.ok(rows.every(r=>!r.includes('2025-12-31')),'no pre-period fake zero');
 await page.locator('#fd-profit-ranges [data-range="1d"]').click();await page.waitForTimeout(50);assert.ok((await page.locator('#fd-profit-tbody').textContent()).includes('12.00'));
 for(const range of ['7d','1m','3m','6m','1y']){
  await page.locator(`#fd-profit-ranges [data-range="${range}"]`).click();
  await page.waitForTimeout(30);
  assert.equal(await page.locator(`#fd-profit-ranges [data-range="${range}"]`).getAttribute('aria-pressed'),'true');
 }
 await page.locator('#fd-tab-performance').click();
 for(const range of ['1d','7d','1m','3m','6m','1y']){
  await page.locator(`#fd-perf-ranges [data-range="${range}"]`).click();
  await page.waitForTimeout(30);
  assert.equal(await page.locator(`#fd-perf-ranges [data-range="${range}"]`).getAttribute('aria-pressed'),'true');
  assert.match(await page.locator('#fd-perf-tbody').textContent(),/2026-01-07/);
  if(range==='1d')assert.equal(await page.locator('#fd-perf-tbody tr').count(),1);
 }
 await page.evaluate(()=>{window.quarterFixture={report_date:'2025-12-31',previous_report_date:'2025-09-30',stocks:Array.from({length:61},(_,i)=>({code:String(i+1).padStart(6,'0'),name:'Stock '+i,quoteSymbol:'sz'+String(i+1).padStart(6,'0'),ratio_pct:'6.45',previous_ratio_pct:i===60?null:'4.31',change_pct:'2.14'}))};Fund.openFundDetail(holding);});
 await page.waitForFunction(()=>document.querySelectorAll('#fd-quarterly-tbody tr').length===61&&document.getElementById('fd-quarterly-tbody').lastElementChild.textContent.includes('1.25%'));
 assert.match(await page.locator('#fd-quarterly-status').textContent(),/2025-12-31.*2025-09-30/);
 assert.match(await page.locator('#fd-quarterly-tbody tr').first().textContent(),/\+2.14/);
 assert.match(await page.locator('#fd-quarterly-tbody tr').last().textContent(),/fundDetailPreviousUnknown/);
 const batches=await page.evaluate(()=>calls.filter(c=>c.url.startsWith('/api/market/quotes?')).map(c=>new URL(c.url,'http://fixture.test').searchParams.get('codes').split(',').length));
 assert.deepEqual(batches.slice(-2),[60,1],'every disclosed constituent receives quotes, including rows after the first 60');
 await page.evaluate(()=>{window.quarterFixture=null;});
 await page.evaluate(()=>{mode='percent';document.dispatchEvent(new Event('fund:privacy-changed'));});assert.ok(!(await page.locator('#fd-val-shares').textContent()).includes('12'),'percentage mode must conceal shares');
 await page.evaluate(()=>{mode='hidden';document.dispatchEvent(new Event('fund:privacy-changed'));});assert.equal(await page.locator('#fd-fund-account').textContent(),'****');
 await page.evaluate(()=>{mode='amount';window.mixedNav=true;Fund.openFundDetail({...holding,fundId:3,fundCode:'000001'});});
 await page.waitForFunction(()=>document.getElementById('fd-head-year-pct').textContent==='fundDetailMixedSources');
 await page.locator('#fd-tab-performance').click();
 assert.ok((await page.locator('#fd-perf-tbody').textContent()).includes('fundDetailMissing'),'different NAV sources must not invent a daily change');
 await page.evaluate(()=>{window.mixedNav=false;});
 await page.evaluate(()=>{
  mode='amount';document.dispatchEvent(new Event('fund:privacy-changed'));
  window.pauseNavCode='110022';
  Fund.openFundDetail(holding);
 });
 await page.locator('#fd-tab-profit').click();
 await page.evaluate(()=>Fund.openFundDetail({...holding,fundId:3,fundCode:'000001',fundName:'Second fund',account:'Second account'}));
 assert.equal(await page.locator('#fd-tab-holdings').getAttribute('aria-selected'),'true');
 assert.equal(await page.locator('#fd-profit-ranges [data-range="1m"]').getAttribute('aria-pressed'),'true');
 await page.evaluate(()=>lateNav({nav:[{navDate:'2026-01-07',unitNav:'999'}],nextCursor:null}));
 await page.waitForTimeout(50);
 assert.equal(await page.locator('#fd-fund-name').textContent(),'Second fund');
 await page.locator('#fd-tab-performance').click();await page.waitForTimeout(50);
 assert.ok(!(await page.locator('#fd-perf-tbody').textContent()).includes('999'),'late first holding NAV must not overwrite the second');
 await page.evaluate(()=>{window.pauseNavCode=null;window.failQuarterCode='110022';Fund.openFundDetail(holding);Fund.openFundDetail({...holding,fundId:3,fundCode:'000001'});lateQuarterError(Error('old quarter failure'));});
 await page.waitForTimeout(50);
 assert.ok(!(await page.locator('#fd-quarterly-tbody').textContent()).includes('old quarter failure'),'late failure must not replace current holding data');
 await page.evaluate(()=>Fund.leaveFundDetail());
 assert.equal(await page.locator('#view-fund-detail').isVisible(),false);
 assert.deepEqual(errors,[]);writeFileSync(resolve(out,'fund-browser.json'),JSON.stringify({checks:['historical quantities and fees','account-scoped history endpoint','NAV and profit six ranges','single-day real profit','no pre-period zero','percentage/hidden privacy','quarter dates and unknown previous weight','61 constituent quotes in bounded batches','late response identity isolation'],errors},null,2));console.log('fund detail: income contract, six ranges, quarterly comparison and quote batches, privacy and late replies passed');
}finally{await browser.close();}
