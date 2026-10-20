// Isolated real Runtime and ledger; external fund sources may explicitly fail.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
export async function verifyInvestmentRemediation(page,frame,report) {
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 const out=resolve(import.meta.dirname,'../../.tmp/investment-final-fixes/native');mkdirSync(out,{recursive:true});
 const fixture=await runtime.evaluate(async()=>{
  const date=offset=>new Date(Date.now()+8*3600000-offset*86400000).toISOString().slice(0,10);
  const readDetail=async()=>{for(let attempt=0;;attempt++){try{return await Fund.api('GET','/api/positions/detail');}catch(error){if(error.status!==409||attempt===5)throw error;await new Promise(resolve=>setTimeout(resolve,100));}}};
  await Fund.api('POST','/api/openings',{account:'Detail isolated',instrumentKind:'fund',fundCode:'022365',fundName:'Detail fund',quantity:'10',totalCost:'10',asOf:date(3)});
  const detail=await readDetail(),h=detail.holdings.find(h=>h.fundCode==='022365');
  for(let n=3;n>=0;n--){const snapshot=await readDetail();await Fund.api('POST','/api/holding/valuation',{fundId:h.fundId,dataRevision:snapshot.dataRevision,price:String(1+(3-n)/10),asOf:date(n),navCurrency:'CNY',riskClass:'equity'});}
  return {...h,today:date(0)};
 });
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 await frame.locator('#tab-positions').click();await frame.locator('#pos-tab-all').click();
 const card=frame.locator(`[data-holding-id="${fixture.accountId}:${fixture.fundId}"]`);
 report.entry=await runtime.evaluate(()=>({openFundDetail:typeof Fund.openFundDetail,mode:Fund.home.displayMode(),sensitive:Fund.home.sensitive(),visibility:document.visibilityState,focus:document.hasFocus()}));
 await card.locator('.holding-detail-link').click();
 await frame.locator('#view-fund-detail').waitFor({state:'visible',timeout:10000});
 assert.match(await frame.locator('#fd-val-amount').innerText(),/13\.00/);
 assert.match(await frame.locator('#fd-val-shares').innerText(),/^10$/);
 await runtime.waitForFunction(()=>document.getElementById('fd-val-ratio').textContent==='100.00%');
 assert.match(await frame.locator('#fd-val-profit').innerText(),/3\.00/);
 assert.match(await frame.locator('#fd-val-day-income').innerText(),/1\.00/);
 assert.doesNotMatch(await frame.locator('#fd-head-year-pct').innerText(),/%/,'short or mixed manual/public NAV must not claim a yearly change');
 assert.match(await frame.locator('#fd-label-day-income').innerText(),new RegExp(fixture.today));
 await frame.locator('#fd-tab-profit').click();await frame.locator('#fd-profit-ranges [data-range="1d"]').click();
 await runtime.waitForFunction(()=>document.getElementById('fd-profit-tbody').textContent.includes('1.00'),null,{timeout:30000});
 const source=await runtime.evaluate(()=>Fund.api('GET','/api/positions/detail'));
 report.steps.push({name:'Real account-scoped fund detail summary and single-day historical income',dataRevision:source.dataRevision});
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
  await page.setViewportSize({width,height:900});
  await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));},{theme,language});
  assert.equal(await runtime.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  assert.equal(await frame.locator('.fund-detail-topbar .sd-topbar-right').evaluate(el=>[...el.querySelectorAll('button')].every(b=>{const a=el.getBoundingClientRect(),r=b.getBoundingClientRect();return r.left>=a.left-1&&r.right<=a.right+1;})),true);
  assert.doesNotMatch(await frame.locator('#view-fund-detail').innerText(),/fundDetail[A-Z]/);
  assert.equal(await frame.locator('.fund-section-tabs').evaluate(el=>[...el.querySelectorAll('button')].every(b=>{const a=el.getBoundingClientRect(),r=b.getBoundingClientRect();return r.left>=a.left-1&&r.right<=a.right+1;})),true);
  await runtime.evaluate(()=>{document.querySelector('.fund-detail-workspace').scrollTop=0;});
  await frame.locator('#view-fund-detail').screenshot({path:resolve(out,`detail-${theme}-${language}-${width}.png`)});
 }
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('percent',false));assert.equal(await frame.locator('#fd-val-shares').textContent(),'****');
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));assert.equal(await frame.locator('#fd-fund-account').textContent(),'****');
 assert.equal(await runtime.evaluate(()=>!!echarts.getInstanceByDom(document.getElementById('fd-profit-chart'))),false);
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));await page.setViewportSize({width:1280,height:900});
 await frame.locator('#fd-btn-back').click();await card.waitFor();
 await runtime.waitForFunction(id=>document.activeElement.id===id,'pos-detail-'+fixture.accountId+'-'+fixture.fundId);
 assert.equal(await frame.locator('#view-fund-detail').isVisible(),false);
 report.steps.push({name:'8 fund-detail theme/language/viewport cases, privacy and return to holding card'});
 const publicHolding=await runtime.evaluate(async()=>{
  const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Public quarter isolated',instrumentKind:'fund',fundCode:'000001',fundName:'Public quarterly probe',quantity:'10',totalCost:'10',asOf:date});
  const data=await Fund.api('GET','/api/positions/detail');return data.holdings.find(h=>h.fundCode==='000001');
 });
 await frame.locator('#pos-refresh').click();
 await frame.locator(`[data-holding-id="${publicHolding.accountId}:${publicHolding.fundId}"] .holding-detail-link`).click();
 await runtime.waitForFunction(()=>document.querySelectorAll('#fd-quarterly-tbody [data-stock]').length>0,null,{timeout:30000});
 const disclosure=await runtime.evaluate(()=>Fund.api('GET','/api/fund/quarterly-positions?code=000001'));
 assert.ok(disclosure.previous_report_date<disclosure.report_date);
 assert.equal(await frame.locator('#fd-quarterly-tbody [data-stock]').count(),disclosure.stocks.length+disclosure.bonds.length);
 assert.match(await frame.locator('#fd-quarterly-status').innerText(),new RegExp(disclosure.report_date));
 assert.match(await frame.locator('#fd-quarterly-status').innerText(),new RegExp(disclosure.previous_report_date));
 const comparable=disclosure.stocks.find(s=>s.previous_ratio_pct!=null);
 assert.ok(comparable);
 const unit=await runtime.evaluate(()=>Fund.t('fundDetailPercentagePoints')),delta=Number(comparable.change_pct);
 assert.equal(await frame.locator(`[data-stock="${comparable.code}"]`).locator('xpath=ancestor::tr').locator('td').nth(3).innerText(),(delta>0?'+':'')+delta.toFixed(2)+' '+unit);
 await frame.locator('#fd-quarterly-tbody [data-stock]').first().click();
 await frame.locator('#fd-stock-minute-chart canvas').first().waitFor({timeout:30000});
 report.steps.push({name:'Actual public fund two-period disclosure in private detail, all published rows, comparable percentage points and selected latest-trading-day chart',reportDate:disclosure.report_date,previousReportDate:disclosure.previous_report_date,stocks:disclosure.stocks.length,bonds:disclosure.bonds.length});
 await frame.locator('#fd-btn-back').click();
}

export async function verifyStockDetails(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 const accounts=await runtime.evaluate(async()=>{
  const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  for(const [account,quantity,cost] of [['Stock A','10','100'],['Stock B','20','400']])await Fund.api('POST','/api/openings',{account,instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Synthetic stock',quantity,totalCost:cost,asOf:today});
  for(let n=0;n<21;n++)await Fund.api('POST','/api/transactions',{account:'Stock A',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Synthetic stock',type:'BUY',state:'confirmed',quantity:'1',price:'1',fee:'0',tradeDate:today,requestId:'stock-record-page-'+n});
  const snapshot=await Fund.api('GET','/api/positions/detail');return snapshot.holdings.map(h=>({...h,account:snapshot.accounts.find(a=>a.id===h.accountId).name}));
 });
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));await frame.locator('#tab-positions').click();await frame.locator('#pos-tab-all').click();
 const first=accounts.find(h=>h.account==='Stock A'),second=accounts.find(h=>h.account==='Stock B');
 await frame.locator(`[data-holding-id="${first.accountId}:${first.fundId}"] .holding-detail-link`).click();
 await frame.locator('#view-stock-detail').waitFor();assert.match(await frame.locator('#sd-holding-context').innerText(),/Stock A/);
 assert.match(await frame.locator('#sd-holding-context').innerText(),/31/);
 await frame.locator('[data-holding-records]').click();await frame.locator('#sd-holding-panel tbody tr').first().waitFor();
 assert.equal(await frame.locator('#sd-holding-panel tbody tr').count(),20);await frame.locator('[data-record-dir=next]').click();
 await runtime.waitForFunction(()=>document.querySelector('#sd-holding-panel nav span').textContent==='2');assert.equal(await frame.locator('#sd-holding-panel tbody tr').count(),1);
 await frame.locator('button[data-holding-tab=overview]').click();assert.equal(await frame.locator('button[data-holding-tab=overview]').getAttribute('aria-selected'),'true');
 await frame.locator('#tab-positions').click();await frame.locator(`[data-holding-id="${second.accountId}:${second.fundId}"] .holding-detail-link`).click();
 assert.match(await frame.locator('#sd-holding-context').innerText(),/Stock B/);assert.ok(!(await frame.locator('#sd-holding-context').innerText()).includes('Stock A'));
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));assert.ok(!(await frame.locator('#sd-holding-context').innerText()).includes('Stock B'));
 report.steps.push({name:'Real stock account identity A/B, 21-record pagination, Escape and hidden privacy',firstAccount:first.accountId,secondAccount:second.accountId});
}
