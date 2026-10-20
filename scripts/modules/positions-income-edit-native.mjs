// 实际 Native/Runtime 隔离账本：收益双值、业务日期、来源编辑与冲突回归。
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
export async function verifyNativeHoldingIncomeEdit(page,frame,report) {
  const out=resolve(import.meta.dirname,'../../.tmp/positions-income-edit');mkdirSync(out,{recursive:true});
  const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
  await page.bringToFront();
  await runtime.waitForFunction(()=>document.visibilityState==='visible');
  await page.evaluate(()=>chrome.storage.local.set({'natives-theme':'archive','natives-language':'zh_CN'}));
  await runtime.waitForFunction(()=>Fund.state.language==='zh-CN');
  const data=await runtime.evaluate(async()=>{
    const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
    const date=n=>new Date(new Date(today+'T12:00:00+08:00').getTime()-n*86400000).toISOString().slice(0,10);
    await Fund.api('POST','/api/openings',{account:'N08 isolated',instrumentKind:'fund',fundCode:'022365',fundName:'N08 fund',quantity:'10',totalCost:'10',asOf:date(3)});
    let detail=await Fund.api('GET','/api/positions/detail');
    const holding=detail.holdings.find(h=>h.fundCode==='022365');
    for(let n=3;n>=0;n--) {
      detail=await Fund.api('GET','/api/positions/detail');
      await Fund.api('POST','/api/holding/valuation',{fundId:holding.fundId,dataRevision:detail.dataRevision,price:String(1+(3-n)/10),asOf:date(n),navCurrency:'CNY',riskClass:'equity'});
    }
    return {accountId:holding.accountId,fundId:holding.fundId,today,yesterday:date(1)};
  });
  await runtime.evaluate(async()=>{
    const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
    const created=await Fund.api('POST','/api/offline/assets',{productType:'gold',name:'Income isolated gold',valueMinor:'12000',costMinor:'10000',valuedAt:date,riskClass:'gold',attrs:{}});
    await Fund.api('POST','/api/offline/assets/exit',{id:created.id,exitDate:date,exitAmountMinor:'5000',exitFeeMinor:'100',remainingValueMinor:'8000',expectedVersion:1,requestId:'income-offline-partial'});
  });
  const reconcile=await runtime.evaluate(async()=>{
    const p=await Fund.api('GET','/api/positions/detail?refresh=1&kind=otc');
    const h=await Fund.api('GET','/api/home/snapshot');
    return {holding:p.holdings[0],ledger:h.portfolio.ledger,net:h.portfolio.dailyReturn,offline:h.portfolio.offlineIncome};
  });
  assert.equal(reconcile.ledger.marketValueMinor,'1300');
  assert.equal(reconcile.ledger.holdingProfitMinor,'300');
  assert.equal(reconcile.ledger.day.amountMinor,'100');
  assert.equal(reconcile.holding.income.day.amount,'1');
  assert.equal(reconcile.ledger.day.date,reconcile.holding.income.day.date);
  await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
  await frame.locator('#home-refresh-btn').click();
  await runtime.waitForFunction(()=>Fund.home.snapshot()?.portfolio.ledger.marketValueMinor==='1300');
  assert.match(await frame.locator('#home-fund-value').innerText(),/13.00/);
  assert.match(await frame.locator('#home-fund-daily').innerText(),/1.00/);
  assert.equal(await frame.locator('.home-income-card').count(),4);
  assert.equal(reconcile.offline.bookProfitMinor,'2900');
  assert.equal(reconcile.offline.receivableMinor,'4900');
  assert.match(await frame.locator('#home-offline-profit').innerText(),/29.00/);
  assert.equal(reconcile.ledger.byType.fund.marketValueMinor,'1300');
  assert.equal(reconcile.ledger.byType.fund.holdingProfitMinor,'300');
  assert.equal(reconcile.ledger.byType.stock.holdingCount,0);
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
    const cdp=await page.context().newCDPSession(page);await cdp.send('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});
    await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));},{theme,language});
    assert.ok(await frame.locator('.home-income-grid').evaluate(el=>parseFloat(getComputedStyle(el).gap)>=8),'income cards retain token spacing');
    assert.match(await frame.locator('#home-fund-profit').innerText(),/3.00/);
    assert.match(await frame.locator('#home-fund-income .home-income-rate').innerText(),/30/);
    assert.equal(await runtime.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
    await frame.locator('.home-income-grid').screenshot({path:resolve(out,`income-${theme}-${language}-${width}.png`)});
    await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
    assert.equal(await frame.locator('#home-fund-profit').innerText(),'****');
    await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));await cdp.detach();
  }
  const cdp=await page.context().newCDPSession(page);await cdp.send('Emulation.clearDeviceMetricsOverride');await cdp.detach();
  await runtime.evaluate(()=>{document.documentElement.dataset.theme='archive';Fund.state.language='zh-CN';document.dispatchEvent(new Event('fund:language-changed'));});
  const refreshed=page.waitForResponse(r=>r.url().includes('/api/home/snapshot/generate')&&r.request().method()==='POST'&&r.status()===200);
  await frame.locator('#btn-manual-refresh').click();
  await refreshed;
  report.steps.push({name:'Home four income cards: classified fund/stock, actual offline partial-exit 29 yuan/49 yuan receivable, 8 theme/language/width and privacy cases; Home/Positions reconcile',reconcile});
  const id=data.accountId+':'+data.fundId;
  await frame.locator('#tab-positions').click();
  await frame.locator('#pos-tab-all').click();
  await runtime.evaluate(async()=>{Fund.home.applyDisplayMode('amount',false);await Fund.actions.loadPositions();});
  const row=frame.locator('[data-pos-act=edit][data-pos-id="'+id+'"]').locator('xpath=ancestor::article');
  await row.waitFor();
  const addCard=frame.locator('[data-pos-add-account="'+data.accountId+'"]');
  for(const theme of ['archive','volt']) for(const language of ['zh-CN','en']) {
    await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));},{theme,language});
    await addCard.focus();await addCard.press('Enter');
    await frame.locator('dialog[open] [name=q]').waitFor();
    await frame.locator('dialog[open]').press('Escape');
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    assert.equal(await addCard.evaluate(el=>document.activeElement===el),true,'add-card cancel restores its trigger');
  }
  await runtime.evaluate(()=>{document.documentElement.dataset.theme='archive';Fund.state.language='zh-CN';document.dispatchEvent(new Event('fund:language-changed'));});
  await row.locator(".pos-card-details > summary").click();
  assert.equal(await row.locator('.pos-income[data-income]').count(),3);
  assert.equal(await frame.locator('[data-metrics-for="'+id+'"] [data-income]').count(),4);
  assert.match(await row.locator('[data-income=day]').innerText(),/1\.00/);
  assert.match(await row.locator('[data-income=day]').innerText(),/8\.33%/);
  assert.match(await row.locator('[data-income=holding]').innerText(),/3\.00/);
  assert.match(await row.locator('[data-income=holding]').innerText(),/30\.00%/);
  assert.match(await row.locator('[data-income=holding]').innerText(),/持有 3 天/);
  const before=await runtime.evaluate(()=>Fund.api('GET','/api/home/snapshot'));
  const edit=frame.locator('[data-pos-act=edit][data-pos-id="'+id+'"]');
  await page.bringToFront();
  await edit.scrollIntoViewIfNeeded();
  await runtime.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  await edit.click();
  await frame.locator('[data-record-opening]').waitFor({timeout:30000});
  await frame.locator('[data-record-opening]').click();
  await frame.locator('dialog[open] [name=quantity]').fill('10.1234');
  await frame.locator('dialog[open] [name=totalCost]').fill('8');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open]').waitFor({state:'detached'});
  const after=await runtime.evaluate(async()=>{
    const detail=await Fund.api('GET','/api/positions/detail');
    const home=await Fund.api('GET','/api/home/snapshot');
    return {holding:detail.holdings.find(h=>h.fundCode==='022365'),home};
  });
  assert.equal(after.holding.quantity,'10.1234');assert.equal(after.holding.cost,'8');
  assert.equal(BigInt(after.home.portfolio.securities.knownCostMinor)-BigInt(before.portfolio.securities.knownCostMinor),-200n);
  await runtime.waitForFunction(id=>document.activeElement.id===id,'pos-edit-'+data.accountId+'-'+data.fundId);
  await edit.click();await frame.locator('[data-record-opening]').click();
  await frame.locator('dialog[open] [name=quantity]').fill('10.2345');
  // 真实第二写入使已打开表单的版本过期；提交必须拒绝并保留草稿。
  await runtime.evaluate(async ({accountId,fundId})=>{
    const source=await Fund.api('GET','/api/holding/records?accountId='+accountId+'&fundId='+fundId);
    await Fund.api('POST','/api/opening/change',{accountId,fundId,op:'update',expectedVersion:source.opening.version,
      quantity:source.opening.quantity,totalCost:'8',asOf:source.opening.asOf});
  },data);
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] .home-form-error:not(:empty)').waitFor();
  assert.equal(await frame.locator('dialog[open] [name=quantity]').inputValue(),'10.2345');
  await page.keyboard.press('Escape');
  await edit.click();await frame.locator('[data-record-opening]').click();
  await frame.locator('dialog[open] [name=totalCost]').fill('8.50');
  await page.evaluate(()=>chrome.storage.local.set({'natives-language':'en'}));
  await runtime.waitForFunction(()=>Fund.state.language==='en');
  assert.equal(await frame.locator('dialog[open] [name=totalCost]').inputValue(),'8.50');
  await page.keyboard.press('Escape');
  for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [320,1280]) {
    await page.setViewportSize({width,height:900});
    await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
    await runtime.waitForFunction(({theme,language})=>document.documentElement.dataset.theme===theme&&Fund.state.language===(language==='en'?'en':'zh-CN'),{theme,language});
    assert.match(await row.locator('[data-income=day]').innerText(),/%/);
    assert.match(await row.locator('[data-income=yesterday]').innerText(),language==='en'?/\d+\/\d+/:/月.*日/);
    const actionVisibility=await edit.evaluate(button=>{
      const action=button.getBoundingClientRect(),container=button.closest('.pos-holding-card').getBoundingClientRect();
      return action.left>=container.left&&action.right<=container.right+1;
    });
    assert.equal(actionVisibility,true,'Edit stays inside its holding card');
    assert.deepEqual(await runtime.evaluate(()=>({scroll:document.documentElement.scrollWidth,width:document.documentElement.clientWidth})).then(m=>m.scroll>m.width+1),false);
    await page.screenshot({path:resolve(out,'income-'+theme+'-'+language+'-'+width+'.png'),animations:'disabled'});
  }
  await runtime.evaluate(()=>Fund.home.applyDisplayMode('percent',false));
  assert.match(await row.locator('[data-income=holding]').innerText(),/\*\*\*\*/);
  assert.match(await row.locator('[data-income=holding]').innerText(),/%/);
  assert.equal(await edit.isDisabled(),true);
  await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
  assert.doesNotMatch(await row.locator('[data-income=holding]').innerText(),/%/);
  await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
  const transaction=await runtime.evaluate(async ({today})=>{
    const tx=await Fund.api('POST','/api/transactions',{account:'N08 isolated',instrumentKind:'fund',fundCode:'022365',
      type:'BUY',state:'confirmed',quantity:'1',price:'1.3',fee:'0',tradeDate:today,requestId:'N08-edit-transaction'});
    await Fund.actions.loadPositions();return tx;
  },data);
  await edit.click();await frame.locator('[data-record-transaction="'+transaction.id+'"]').click();
  await frame.locator('dialog[open] [name=quantity]').fill('1.2345');
  await frame.locator('dialog[open] [name=price]').fill('1.2');
  await frame.locator('dialog[open] [name=reason]').fill('N08 isolated correction');
  await frame.locator('dialog[open] [name=cashConfirmed]').check();
  await frame.locator('dialog[open] [name=cashNote]').fill('N08 isolated receipt');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-save]:not([disabled])').waitFor();
  await frame.locator('dialog[open] [data-save]').click();
  await frame.locator('dialog[open]').waitFor({state:'detached'});
  const corrected=await runtime.evaluate(async data=>{
    const detail=await Fund.api('GET','/api/positions/detail');
    const sources=await Fund.api('GET','/api/holding/records?accountId='+data.accountId+'&fundId='+data.fundId);
    return {holding:detail.holdings.find(h=>h.fundId===data.fundId),sources};
  },data);
  assert.equal(corrected.holding.quantity,'11.3579');
  assert.equal(corrected.sources.transactions.length,1);
  assert.equal(corrected.sources.transactions[0].quantity,'1.2345');
  assert.equal(corrected.sources.transactions[0].price,'1.2');
  await runtime.evaluate(async()=>{
    const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
    await Fund.api('POST','/api/openings',{account:'Cards isolated',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Cards stock',quantity:'10',totalCost:'100',asOf:date});
    await Fund.api('POST','/api/openings',{account:'Cards isolated',instrumentKind:'etf',market:'SSE',fundCode:'510300',fundName:'Cards ETF',quantity:'20',totalCost:'80',asOf:date});
    await Fund.actions.loadPositions();
  });
  await page.setViewportSize({width:1280,height:900});
  const actual=await runtime.evaluate(()=>Fund.api('GET','/api/positions/detail'));
  assert.equal(await frame.locator('.pos-holding-card:not(.pos-add-card)').count(),actual.holdings.length);
  assert.equal(await frame.locator('.pos-table').count(),0);
  for(const kind of ['stock','etf']){
    const h=actual.holdings.find(h=>h.instrumentKind===kind);
    const card=frame.locator('[data-holding-id="'+h.accountId+':'+h.fundId+'"]');
    for(const action of ['buy','sell']){
      const button=card.locator('.pos-actions > [data-pos-act='+action+']');
      await button.scrollIntoViewIfNeeded();await button.click();
      await frame.locator('dialog[open] [name=quantity]').waitFor();
      assert.equal(await frame.locator('dialog[open] [name=quantity]').getAttribute('step'),kind==='stock'?'1':'0.0001');
      if(action==='sell')assert.equal(await frame.locator('dialog[open] [name=quantity]').inputValue(),h.quantity);
      await page.keyboard.press('Escape');
      assert.equal(await runtime.evaluate(()=>document.activeElement.dataset.posAct),action);
    }
  }
  const retained=await runtime.evaluate(()=>Fund.api('GET','/api/positions/detail'));
  for(const h of actual.holdings){const next=retained.holdings.find(n=>n.accountId===h.accountId&&n.fundId===h.fundId);assert.equal(next.quantity,h.quantity);assert.equal(next.cost,h.cost);}
  report.steps.push({name:'holding cards real Native: one per OTC fund/stock/ETF, stock and ETF direct buy/sell dialogs retain precision, cancel restores focus and does not mutate ledger',count:actual.holdings.length});
  await frame.locator('#tab-home').click();await page.setViewportSize({width:1280,height:900});
  report.steps.push({name:'N08 real ledger: three primary and four secondary money/rate cells, actual dates/age; visible opening editor updates quantity/cost and home; confirmed transaction correction updates quantity/price; stale version preserves draft; language draft/Escape/focus, Archive/Volt widths and privacy',accountId:data.accountId});
}
