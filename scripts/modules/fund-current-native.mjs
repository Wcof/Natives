// Live public NAV and (when still pending) live quarterly/quote scenario; isolated ledger only.
import assert from 'node:assert/strict';
export async function verifyCurrentFundData(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await runtime.waitForFunction(()=>Fund.home.snapshot()!=null);
 const result=await runtime.evaluate(async()=>{
  const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10),before=new Date(Date.now()+8*3600000-86400000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Fund current fixture',instrumentKind:'fund',market:'CN',fundCode:'110022',fundName:'Fund source fixture',quantity:'100',totalCost:'100',asOf:before});
  await Fund.api('POST','/api/nav/sync',{fundCode:'110022'});
  Fund.home.applyDisplayMode('amount',false);
  const snapshot=await Fund.api('GET','/api/positions/detail');
  const holding=snapshot.holdings.find(row=>row.fundCode==='110022');
  return {today,snapshot,holding};
 });
 const holding=result.holding;assert.ok(holding);
 const fact=holding.income.day.amount!=null?holding.income.day:holding.income.estimatedDay;
 assert.ok(fact?.amount!=null,'current public data must give confirmed income or an explicitly marked disclosed-holdings reference');
 assert.equal(fact.date,result.today);
 if(holding.nav.date===result.today){assert.ok(holding.nav.dailyGrowthPct!=null);assert.ok(holding.dayPct!=null);}
 else{assert.equal(holding.estimate.status,'estimated');assert.ok(holding.estimate.coverageBps>0);assert.equal(holding.estimate.complete,false);}
 report.fundCurrent={date:result.today,nav:holding.nav,day:holding.income.day,estimated:holding.income.estimatedDay||null,estimate:holding.estimate};
 const shared=await runtime.evaluate(async()=>{await Fund.actions.refreshHomeValuation();const snapshot=Fund.home.snapshot(),ledger=snapshot.portfolio.ledger;const daily=!ledger.day.complete&&ledger.estimatedDay.estimatedCount>0?ledger.estimatedDay:ledger.day;return {amount:Fund.formatMinorExact(BigInt(daily.amountMinor)),themes:await Fund.api('GET','/api/fund/classification?code=512480')};});
 assert.equal(await frame.locator('#home-pending-summary').innerText(),shared.amount,'home summary reads current ledger daily income');
 assert.equal(shared.themes.status,'ready');assert.ok(shared.themes.themes.some(tag=>tag.name==='半导体'));
 report.fundThemes=shared.themes;
 const index=await runtime.evaluate(()=>Fund.api('GET','/api/fund/classification?code=007466'));
 assert.equal(index.status,'ready');assert.deepEqual(index.themes,[],'popular recommendations must not become membership tags');assert.equal(index.indexName,'中证红利低波动指数');
 report.fundIndexClassification=index;
 await frame.locator('#tab-positions').click();
 await frame.locator('.pos-holding-card:not(.pos-add-card)').waitFor({timeout:30000});
 assert.match(await frame.locator('.pos-card-daily [data-income=day]').innerText(),/[0-9]/);
 await frame.locator('.pos-holding-card .fund-theme-tag').first().waitFor({timeout:15000});
 const cardThemes=await frame.locator('.pos-holding-card .fund-theme-tag').allTextContents();
 assert.equal(await frame.locator('#pos-total-day-pnl').innerText(),(Number(shared.amount)>0?'+':'')+shared.amount);
 await frame.locator('.holding-detail-link').first().click();
 await frame.locator('#view-fund-detail:not([hidden])').waitFor();
 assert.match(await frame.locator('#fd-val-day-income').innerText(),/[0-9]/);
 assert.match(await frame.locator('#fd-head-day-pct').innerText(),/%/);
 await frame.locator('#fd-fund-themes .fund-theme-tag').first().waitFor({timeout:15000});
 assert.deepEqual(await frame.locator('#fd-fund-themes .fund-theme-tag').allTextContents(),cardThemes);
 report.steps.push({name:'home daily summary and holdings total agree; real public ETF theme and fund card/detail source tags',dailyAmount:shared.amount,cardThemes});
 report.steps.push({name:'real isolated current fund NAV/growth/income API and holding card/detail projection; published or explicitly estimated branch recorded'});
}
