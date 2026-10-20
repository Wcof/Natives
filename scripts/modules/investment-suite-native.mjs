// 四页 UI／UX：复用真实隔离 Chrome→Native 账本，不读取真实用户投资库。
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const out=resolve(import.meta.dirname,'../../.tmp/investment-ui-audit');
export async function navigateInvestment(frame,view){const button=frame.locator('#tab-'+view);if(!await button.isVisible())await frame.locator('#btn-tools-menu').click();await button.click();}
const runtimeOf=page=>page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
export async function verifyNativeInvestmentEmpty(page,frame,report){
  await frame.locator('#tab-assets').click();
  await frame.locator('#offline-cards .investment-row-empty').waitFor();
  assert.equal(await frame.locator('#offline-add').isVisible(),true);
  assert.equal(await frame.locator('#offline-retry').isVisible(),false);
  await navigateInvestment(frame,'tx');
  await frame.locator('#tx-table .investment-row-empty').waitFor();
  assert.equal(await frame.locator('#tx-retry').isVisible(),false);
  await frame.locator('#tab-home').click();
  report.steps.push({name:'N07 empty real ledger: assets and transactions show separate empty states, asset CTA and no false load error'});
}
export async function verifyNativeInvestmentSuite(page,frame,report){
  mkdirSync(out,{recursive:true});
  const runtime=runtimeOf(page);
  await frame.locator('#tab-positions').click();
  await frame.locator('#pos-tab-all').click();
  await runtime.evaluate(async()=>{Fund.home.applyDisplayMode('amount',false);await Fund.actions.loadPositions();});
  const cards=frame.locator('.pos-holding-card[data-holding-id]');
  const count=await runtime.evaluate(async()=> (await Fund.api('GET','/api/positions/detail')).holdings.length);
  assert.equal(await cards.count(),count);
  assert.equal(await frame.locator('.pos-table').count(),0);
  assert.equal(await cards.first().locator('.pos-value strong').innerText().then(s=>s!=='****'),true);
  assert.equal(await cards.first().locator('.pos-card-details').evaluate(el=>el.open),false);

  await frame.locator('#pos-detailed').check();
  assert.equal(await cards.first().locator('.pos-metrics').isVisible(),true);
  assert.equal(await cards.first().locator('.pos-metrics [data-income]').count(),4);
  await frame.locator('#pos-detailed').uncheck();
  assert.equal(await cards.first().locator('.pos-metrics').isVisible(),false);
  await navigateInvestment(frame,'tx');
  await runtime.evaluate(()=>Fund.actions.loadTx());
  assert.equal(await frame.locator('#tx-entry').evaluate(el=>el.open),false);
  await frame.locator('#tx-entry > summary').press('Enter');
  await frame.locator('#tx-form [name=account]').fill('Acceptance cash');
  assert.equal(await frame.locator('#tx-form [name=fee]').inputValue(),'0.00');
  assert.match(await frame.locator('#tx-form [name=tradeDate]').inputValue(),/^\d{4}-\d{2}-\d{2}$/);
  await frame.locator('#tx-form [name=instrumentKind]').selectOption('stock');
  assert.equal(await frame.locator('#tx-form [name=quantity]').getAttribute('step'),'1');
  await frame.locator('#tx-form [name=fundCode]').fill('600519');
  await frame.locator('#tx-form [name=type]').selectOption('SELL');
  await frame.locator('#tx-form [name=quantity]').fill('999999');
  await frame.locator('#tx-form [name=price]').fill('1');
  await frame.locator('#tx-form [type=submit]').click();
  await frame.locator('#tx-msg.err').waitFor();
  assert.equal(await frame.locator('#tx-form [name=quantity]').inputValue(),'999999','rejected real write preserves draft');
  await frame.locator('#tx-form [name=instrumentKind]').selectOption('fund');
  await frame.locator('#tx-form [name=fundCode]').fill('110022');
  await frame.locator('#tx-form [name=type]').selectOption('BUY');
  await frame.locator('#tx-form [name=quantity]').fill('1.1234');
  await frame.locator('#tx-form [name=price]').fill('1.2345');
  assert.equal(await frame.locator('#tx-form [name=state]').inputValue(),'pending');
  const before=await runtime.evaluate(()=>Fund.api('GET','/api/transactions'));
  await frame.locator('#tx-form [type=submit]').click();
  await frame.locator('#tx-msg.success').waitFor();
  assert.equal(await frame.locator('#tx-msg').getAttribute('data-i18n'),'fundLedgerRecordOk');
  await runtime.waitForFunction(()=>document.querySelector('#tx-table').getAttribute('aria-busy')==='false');
  const after=await runtime.evaluate(()=>Fund.api('GET','/api/transactions'));
  assert.equal(after.transactions.length,before.transactions.length+1);
  const added=after.transactions.find(tx=>!before.transactions.some(old=>old.id===tx.id));
  assert.equal(added.quantity,'1.1234');assert.equal(added.price,'1.2345');assert.equal(added.state,'pending');
  assert.equal(await frame.locator('#tx-entry').evaluate(el=>el.open),false);
  assert.equal(await runtime.evaluate(()=>document.activeElement.matches('#tx-entry > summary')),true);
  await frame.locator('#tx-state-filter').selectOption('pending');
  const pendingRow=frame.locator('#tx-table tbody tr').filter({has:frame.locator('[data-transaction][data-id="'+added.id+'"]')});
  await pendingRow.waitFor();
  assert.equal(await pendingRow.locator('[data-transaction=confirm]').count(),1);
  await frame.locator('#tx-state-filter').selectOption('');
  report.steps.push({name:'N07 real Native: one card per holding, three primary income metrics and four expandable secondary metrics; keyboard record form; real rejected sell keeps draft; OTC four-digit pending entry reaches ledger once; collapse and focus restored',transactionId:added.id});
  for(const theme of ['archive','volt'])for(const language of ['zh_CN','en']){
    await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
    await runtime.waitForFunction(({theme,language})=>document.documentElement.dataset.theme===theme&&Fund.state.language===(language==='en'?'en':'zh-CN'),{theme,language});
    for(const width of [320,640,1280]){
      await page.setViewportSize({width,height:900});
      await page.bringToFront();
      await page.waitForFunction(()=>document.visibilityState==='visible');
      for(const view of ['home','positions','assets','tx']){
        await navigateInvestment(frame,view);
        if(view==='assets')await runtime.evaluate(()=>Fund.actions.loadAssets());
        if(view==='tx')await runtime.evaluate(()=>Fund.actions.loadTx());
        await runtime.waitForFunction(view=>!document.getElementById('view-'+view).hidden&&(document.getElementById('tab-'+view).getAttribute('aria-selected')==='true'||document.getElementById('tab-'+view).hasAttribute('data-active')),view).catch(async error=>{
          report.navigationFailure={theme,language,width,view,state:await runtime.evaluate(()=>({visibility:document.visibilityState,active:document.activeElement?.id,views:[...document.querySelectorAll('.main-column > section')].map(el=>({id:el.id,hidden:el.hidden})),tabs:[...document.querySelectorAll('[role=tab]')].map(el=>({id:el.id,selected:el.getAttribute('aria-selected')}))}))};throw error;
        });
        await runtime.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
        const layout=await runtime.evaluate(view=>{
          const target=document.getElementById('view-'+view),main=document.querySelector('.main-column');
          return {width:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth,mainWidth:main.clientWidth,mainScroll:main.scrollWidth,viewWidth:target.clientWidth,viewScroll:target.scrollWidth};
        },view);
        if(layout.viewScroll>layout.viewWidth+1){
          (report.layoutFailures||=[]).push({theme,language,width,view,layout,offenders:await runtime.evaluate(view=>{
            const target=document.getElementById('view-'+view),edge=target.getBoundingClientRect().right;
            return [...target.querySelectorAll('*')].filter(el=>el.getBoundingClientRect().right>edge+1).slice(0,20).map(el=>({tag:el.tagName,class:el.className,id:el.id,text:el.textContent.slice(0,100),width:el.getBoundingClientRect().width,whiteSpace:getComputedStyle(el).whiteSpace}));
          },view)});
          await page.screenshot({path:resolve(out,'overflow-'+view+'-'+theme+'-'+language+'.png')});
        }
        assert.ok(layout.scroll<=layout.width+1,JSON.stringify({theme,language,width,view,layout}));
        assert.ok(layout.mainScroll<=layout.mainWidth+1,JSON.stringify({theme,language,width,view,layout}));
        assert.ok(layout.viewScroll<=layout.viewWidth+1,JSON.stringify({theme,language,width,view,layout}));
        if(width===1280)assert.deepEqual(await runtime.evaluate(view=>[...document.querySelectorAll('#view-'+view+' button')].filter(b=>b.getBoundingClientRect().width>0&&b.getBoundingClientRect().height<31.9).map(b=>b.id||b.textContent),view),[],'32px visible button target '+view);
        if(view!=='home'){
          const text=await frame.locator('#view-'+view).innerText();
          assert.doesNotMatch(text,/undefined|fundHome[A-Z]|fundLedger[A-Z]/);
          if(language==='en')assert.doesNotMatch(text,/[\u4e00-\u9fff]/);
        }
        if(view==='tx')assert.equal(await frame.locator('#tx-msg').innerText(),await runtime.evaluate(()=>Fund.t('fundLedgerRecordOk')),'record feedback follows current language');
        if(width===1280||width===320)await page.screenshot({path:resolve(out,'native-'+view+'-'+theme+'-'+language+'-'+width+'.png'),animations:'disabled'});
      }
    }
  }
  await page.setViewportSize({width:1280,height:900});
  await page.setViewportSize({width:640,height:900});
  await runtime.evaluate(()=>document.body.style.zoom='2');
  for(const view of ['home','positions','assets','tx']){
    await navigateInvestment(frame,view);
    assert.ok(await runtime.evaluate(()=>document.querySelector('.main-column').scrollWidth<=document.querySelector('.main-column').clientWidth+1),'200% page width '+view);
  }
  await runtime.evaluate(()=>document.body.style.zoom='');
  await page.setViewportSize({width:1280,height:900});
  await page.evaluate(()=>chrome.storage.local.set({'natives-theme':'archive','natives-language':'en'}));
  await frame.locator('#tab-home').click();
  await frame.locator('#tab-home').focus();
  await page.keyboard.press('ArrowRight');
  assert.equal(await frame.locator('#tab-positions').getAttribute('aria-selected'),'true');
  await page.keyboard.press('ArrowRight');
  assert.equal(await frame.locator('#tab-assets').getAttribute('aria-selected'),'true');
  await frame.locator('#offline-add').click();
  await frame.locator('dialog[open] [name=name]').fill('Unsaved UI audit draft');
  await frame.locator('dialog[open] [name=value]').fill('123.45');
  await page.evaluate(()=>chrome.storage.local.set({'natives-language':'zh_CN'}));
  await runtime.waitForFunction(()=>Fund.state.language==='zh-CN'&&document.querySelector('dialog[open] h2')?.textContent===Fund.t('fundHomeAddOffline'));
  assert.equal(await frame.locator('dialog[open] [name=name]').inputValue(),'Unsaved UI audit draft');
  await page.keyboard.press('Escape');
  assert.equal(await runtime.evaluate(()=>document.activeElement.id),'offline-add');
  await page.evaluate(()=>chrome.storage.local.set({'natives-language':'en'}));
  await runtime.waitForFunction(()=>Fund.state.language==='en');
  await frame.locator('#tab-home').click();
  await runtime.evaluate(()=>Fund.home.refresh());
  if(!await frame.locator('.home-extra-research').evaluate(n=>n.open)){await frame.locator('.home-extra-research > summary').focus();await frame.locator('.home-extra-research > summary').press('Enter');}
  if(!await frame.locator('.opportunity-filter-more').evaluate(node=>node.open)){await frame.locator('.opportunity-filter-more > summary').focus();await frame.locator('.opportunity-filter-more > summary').press('Enter');}
  report.filterDisclosure=await runtime.evaluate(()=>({parentOpen:document.querySelector('.home-extra-research').open,advancedOpen:document.querySelector('.opportunity-filter-more').open,focus:document.activeElement.outerHTML.slice(0,240)}));
  assert.equal(report.filterDisclosure.parentOpen,true);
  assert.equal(report.filterDisclosure.advancedOpen,true);
  await frame.locator('[data-opportunity-filters] [name=market]').selectOption('CN');
  await frame.locator('[data-opportunity-filters] [name=minScore]').fill('0');
  await frame.locator('[data-opportunity-filters] [name=minScore]').focus();
  await runtime.evaluate(()=>Fund.home.renderOpportunities(Fund.home.snapshot().opportunities));
  assert.equal(await frame.locator('.opportunity-filter-more').evaluate(el=>el.open),true);
  assert.equal(await frame.locator('[data-opportunity-filters] [name=market]').inputValue(),'CN');
  assert.equal(await runtime.evaluate(()=>document.activeElement.name),'minScore');
  await frame.locator('[data-opportunity-filters] [data-opportunity-reset]').click();
  if(!await frame.locator('.opportunity-filter-more').evaluate(node=>node.open)){await frame.locator('.opportunity-filter-more > summary').focus();await frame.locator('.opportunity-filter-more > summary').press('Enter');}
  report.steps.push({name:'N07 four real views: Archive/Volt, Chinese/English, 320/640/1280; no page overflow or missing copy; primary nav arrows; asset dialog draft/language/Escape/focus; advanced home filters preserve draft/open/focus; screenshots'});
}

// ==== mode 复核（2026-10-05）：图形/自选真实读写与 Model Settings 入口（N 层） ====
export async function verifyNativeDrawingsWatchlist(page, frame, report) {
  const runtime = runtimeOf(page);
  // 图形：save → list 持久化 → 同 id 再存（编辑语义）→ delete → 错误路径 400。
  const drawing = await runtime.evaluate(async () => {
    const save = (body) => Fund.api('POST', '/api/drawings', body);
    const list = (symbol) => Fund.api('GET', '/api/drawings?symbol=sh600519&period=day');
    await save({ id: 'mode-d1', symbol: 'sh600519', period: 'day', toolType: 'trendline', points: [{ date: '2026-09-01', price: 2 }, { date: '2026-09-02', price: 3 }], options: { coordinateVersion: 2, viewContext: { fq: 'qfq', seriesKind: 'OHLC' }, color: '#cdf24b' } });
    const first = await list();
    await save({ id: 'mode-d1', symbol: 'sh600519', period: 'day', toolType: 'trendline', expectedDigest: first.drawings[0].digest, points: [{ date: '2026-09-01', price: 9 }, { date: '2026-09-02', price: 10 }], options: { coordinateVersion: 2, viewContext: { fq: 'qfq', seriesKind: 'OHLC' } } });
    const edited = await list();
    let illegalStatus = null;
    try { await save({ id: 'mode-bad', symbol: 'sh600519', period: 'hour', toolType: 'trendline', points: [] }); } catch (e) { illegalStatus = e.status; }
    return {
      firstCount: first.drawings.length,
      firstPoints: first.drawings[0].points.length,
      editedPoints: edited.drawings[0].points.length,
      updatedAtMoved: edited.drawings[0].updatedAt >= first.drawings[0].updatedAt,
      illegalStatus,
    };
  });
  assert.equal(drawing.firstCount, 1, 'drawing persisted through real native chain');
  assert.equal(drawing.firstPoints, 2);
  assert.equal(drawing.editedPoints, 2, 'same-id save replaces points (edit semantics)');
  assert.equal(drawing.updatedAtMoved, true);
  assert.equal(drawing.illegalStatus, 400, 'unsupported period rejected 400');
  await runtime.evaluate(async () => { const d=(await Fund.api('GET','/api/drawings?symbol=sh600519&period=day')).drawings[0]; await Fund.api('DELETE', '/api/drawings?id=mode-d1&expectedDigest='+d.digest); });
  const emptied = await runtime.evaluate(async () => (await Fund.api('GET', '/api/drawings?symbol=sh600519&period=day')).drawings.length);
  assert.equal(emptied, 0, 'deleted drawing gone');

  // 自选：add → UI 刷新后可见 → 重复 add（upsert created_at 不变）→ remove → 空。
  const watchlist = await runtime.evaluate(async () => {
    const probe = { href: location.href, visible: document.visibilityState };
    let firstPost;
    try {
      firstPost = await Fund.api('POST', '/api/watchlists/items', { watchlistId: 'default', symbol: 'SH600519', name: '贵州茅台' });
    } catch (e) {
      let health = null;
      try { health = await fetch('/healthz').then(r => r.status); } catch (e2) { health = 'fetch-failed: ' + e2.message; }
      throw new Error(`watchlist POST failed: ${e.message} | probe=${JSON.stringify(probe)} health=${health}`);
    }
    void firstPost;
    await Fund.actions.refreshWatchlist?.();
    const first = await Fund.api('GET', '/api/watchlists');
    const created = first.items.find(i => i.symbol === 'sh600519');
    const createdAt = created && created.createdAt;
    await Fund.api('POST', '/api/watchlists/items', { watchlistId: 'default', symbol: 'sh600519', name: '茅台', sortOrder: 3 });
    const second = await Fund.api('GET', '/api/watchlists');
    const afterUpsert = second.items.find(i => i.symbol === 'sh600519');
    let illegalStatus = null;
    try { await Fund.api('POST', '/api/watchlists/items', { watchlistId: 'default', symbol: 42 }); } catch (e) { illegalStatus = e.status; }
    return { createdAt, count: first.items.length, nameAfterUpsert: afterUpsert && afterUpsert.name, sortAfterUpsert: afterUpsert && afterUpsert.sortOrder, createdAtStable: afterUpsert && afterUpsert.createdAt === createdAt, illegalStatus };
  });
  assert.equal(watchlist.count >= 1, true, 'watchlist item added through real native chain');
  assert.equal(watchlist.nameAfterUpsert, '茅台', 'upsert updates name');
  assert.equal(watchlist.sortAfterUpsert, 3, 'upsert updates sort order');
  assert.equal(watchlist.createdAtStable, true, 'upsert does not rewrite created_at');
  assert.equal(watchlist.illegalStatus, 400, 'non-string symbol rejected 400');
  await frame.locator('#tab-watchlist').click();
  await runtime.evaluate(() => Fund.actions.refreshWatchlist());
  await frame.locator('#watchlist-table tbody tr').filter({hasText:'茅台'}).first().waitFor({state:'visible'});
  const rowVisible = await frame.locator('#watchlist-table tbody tr').filter({ hasText: '茅台' }).count();
  assert.equal(rowVisible >= 1, true, 'watchlist UI reflects real data after refresh');
  await runtime.evaluate(async () => { await Fund.api('DELETE', '/api/watchlists/items?watchlistId=default&symbol=sh600519'); await Fund.actions.refreshWatchlist?.(); });
  await frame.locator('#watchlist-table tbody tr').filter({hasText:'茅台'}).waitFor({state:'detached'});
  const rowsAfterRemove = await frame.locator('#watchlist-table tbody tr').filter({ hasText: '茅台' }).count();
  assert.equal(rowsAfterRemove, 0, 'removed item leaves the watchlist UI');
  // 实际画布手势，读取同一隔离账本确认保存、属性编辑与删除。
  await runtime.evaluate(() => Fund.actions.openStockDetail('sh600519', '贵州茅台'));
  await frame.locator('#sd-view-dayk').click();
  await runtime.waitForFunction(() => {
    const chart = globalThis.echarts?.getInstanceByDom(document.getElementById('sd-kline-chart'));
    return chart?.getOption().series?.some(series => series.type === 'candlestick' && series.data?.length);
  }, null, {timeout:30000});
  await frame.locator('.sd-tool-btn[data-tool="horizontal"]').click();
  const chart = frame.locator('#sd-kline-chart');
  const bounds = await chart.boundingBox();
  await chart.click({position:{x:bounds.width*0.5,y:bounds.height*0.4}});
  await runtime.waitForFunction(async () => (await Fund.api('GET','/api/drawings?symbol=sh600519&period=day')).drawings.length === 1);
  const saved = await runtime.evaluate(async () => (await Fund.api('GET','/api/drawings?symbol=sh600519&period=day')).drawings[0]);
  assert.equal(saved.toolType, 'horizontal');
  assert.ok(Number.isFinite(saved.points[0].price));
  const pixel = await runtime.evaluate(point => globalThis.echarts.getInstanceByDom(document.getElementById('sd-kline-chart')).convertToPixel({xAxisIndex:0,yAxisIndex:0},[point.date,point.price]), saved.points[0]);
  await chart.click({button:'right',position:{x:pixel[0],y:pixel[1]}});
  await frame.locator('#sd-line-edit-pop').waitFor({state:'visible'});
  await frame.locator('#sd-lep-name').fill('Isolated drawing acceptance');
  await frame.locator('#sd-lep-save').click();
  await runtime.waitForFunction(async () => (await Fund.api('GET','/api/drawings?symbol=sh600519&period=day')).drawings[0]?.options.name === 'Isolated drawing acceptance');
  await chart.click({button:'right',position:{x:pixel[0],y:pixel[1]}});
  await frame.locator('#sd-lep-del').click();
  await runtime.waitForFunction(async () => (await Fund.api('GET','/api/drawings?symbol=sh600519&period=day')).drawings.length === 0);
  await frame.locator('#tab-home').click();
  report.steps.push({ name: 'mode recheck: real canvas draw → persisted points → context-menu property edit → delete; drawings and watchlist API failure paths' });
}

export async function verifyNativeModelSettingsEntry(page, frame, report, entry = 'standalone app') {
  // app.html 入口：模块 iframe 财务设置 → Model 按钮 → broker(natives:model-settings)
  // → Shell 注入的 openSettings → 真实 Model Settings 对话框；Escape 关闭回焦点。
  if (entry !== 'Workspace embedded') await page.locator('#app-module-settings').click();
  else {
    await page.locator('#settings-entry').click();
    await page.locator('[data-module-settings-app="fund"]').click();
  }
  await frame.locator('#financial-settings-categories [data-category=ai]').click();
  const modelButton = frame.locator('#financial-settings-body [data-settings-category="ai"] [data-settings-action="model"]');
  await modelButton.waitFor({ timeout: 15000 });
  await modelButton.click();
  const dialog = page.locator('.model-settings-dialog[open]');
  await dialog.waitFor({ timeout: 15000 });
  await dialog.locator('#model-settings-title').waitFor();
  assert.ok((await dialog.innerText()).trim().length > 0, 'real Model Settings content required');
  await page.keyboard.press('Escape');
  await page.locator('.model-settings-dialog').waitFor({state:'hidden'});
  await runtimeOf(page).waitForFunction(() => document.activeElement?.matches('[data-settings-action="model"]'), null, {timeout:5000});
  assert.equal(await modelButton.evaluate(el => document.activeElement === el), true, 'module trigger focus restored');
  report.steps.push({ name: `mode recheck: ${entry} Model button → real Model Settings → Escape → trigger focus` });
}

export async function verifyNativeEmbeddedSettings(context, space, report) {
  const base = `${new URL(space.url()).protocol}//${new URL(space.url()).host}`;
  const apps = await context.newPage();
  try {
    await apps.goto(`${base}/apps.html`);
    await apps.locator('.app-card[data-app-id="fund"] .action.primary').click({timeout:30000});
    const frame = apps.frameLocator('iframe');
    await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
    await verifyNativeModelSettingsEntry(apps, frame, report, 'App Center embedded');
  } finally { await apps.close(); }
  await space.locator('.InvestmentGoal .inv-card-open').first().click();
  const frame = space.frameLocator('iframe');
  await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
  await verifyNativeModelSettingsEntry(space, frame, report, 'Workspace embedded');
  await space.locator('.ws-select').filter({hasText:'Isolated investment acceptance'}).click();
  await space.locator('#app-stage iframe').waitFor({state:'detached'});
  await space.locator('.InvestmentGoal .inv-card-open').first().waitFor();
  report.steps.push({name:'mode recheck: embedded module released before return to real Workspace'});
}

export async function verifyNativeTrends(page, report) {
  const frame=page.frameLocator('iframe');
  await frame.locator('#hc-total-value').waitFor({state:'attached',timeout:30000});
  await frame.locator('#tab-trends').click();
  let ready=false;
  for(let i=0;i<24&&!ready;i++) {
    await page.waitForTimeout(500);
    const runtime=runtimeOf(page);
    ready=runtime&&await runtime.evaluate(()=>typeof window.echarts!=='undefined').catch(()=>false);
  }
  assert.ok(ready,'echarts must be lazily injected after entering trends');
  assert.equal(await frame.locator('#kline-chart canvas').count(),0,'empty research must not select an arbitrary instrument');
  await frame.locator('#search-input').fill('600519');
  await frame.locator('#search-results button').first().waitFor({timeout:30000});
  await frame.locator('#search-results button').first().click();
  await frame.locator('#kline-chart canvas').first().waitFor({timeout:30000});
  await runtimeOf(page).waitForFunction(()=>Fund.trends.state.klineData.length>0,{timeout:30000});
  report.steps.push({name:'trends explicit search selection lazily initializes chart and renders actual upstream bars'});
  await runtimeOf(page).evaluate(()=>Fund.home.applyDisplayMode('amount',false));
  await frame.locator('#trends-ai-review').click();
  await frame.locator('dialog[open] [name=scopeAsset]').waitFor();
  assert.equal(await frame.locator('dialog[open] [name=scopeAsset]').inputValue(),'');
  assert.equal(await frame.locator('dialog[open] [data-send]').isDisabled(),true);
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-send]:enabled').waitFor({timeout:30000});
  const preview=JSON.parse(await frame.locator('dialog[open] [data-preview] details pre').textContent());
  assert.equal(preview.scope.research,'sh600519');assert.deepEqual(preview.evidence.map(item=>item.id),['E1','R1']);assert.equal(preview.evidence[1].untrusted,true);
  assert.ok(preview.evidence[1].rows.length>0&&preview.evidence[1].rows.length<=60);
  await frame.locator('dialog[open] [data-send]').click();
  await frame.locator('dialog[open] [data-save-report]:enabled').waitFor({timeout:30000});
  assert.match(await frame.locator('dialog[open] [data-result]').innerText(),/process-model/);
  await runtimeOf(page).evaluate(()=>Fund.trends.charts.klineChart.dispatchAction({type:'dataZoom',start:10,end:80}));
  assert.equal(await frame.locator('dialog[open] [data-send]').isDisabled(),true);
  assert.equal(await frame.locator('dialog[open] [data-save-report]').isDisabled(),true);
  await page.keyboard.press('Escape');
  report.steps.push({name:'public chart AI: real evidence preview, E1/R1 only, actual Model Host/local synthetic provider and viewport invalidation',realProvider:false});
  await frame.locator('#search-input').focus();
  await runtimeOf(page).evaluate(()=>Fund.actions.openStockDetail('jj110022','Isolated NAV','fund'));
  await runtimeOf(page).waitForFunction(()=>Fund.StockDetailShared.sdState.navData.length>=4);
  assert.equal(await frame.locator('#sd-view-minute').isDisabled(),true);
  assert.equal(await frame.locator('#sd-draw-toolbar').isVisible(),false);
  await frame.locator('[data-reader=sd][data-chart-action=table]').click();
  assert.ok(await frame.locator('#sd-data-body tr').count()>=4);
  assert.match(await frame.locator('#sd-data-head').innerText(),/NAV/);
  assert.doesNotMatch(await frame.locator('#sd-data-head').innerText(),/开盘|Open/);
  await page.keyboard.press('Escape');
  const separator=frame.locator('#sd-side-resizer');await separator.focus();
  const original=await runtimeOf(page).evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width'));
  await page.keyboard.press('ArrowLeft');
  assert.notEqual(await runtimeOf(page).evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width')),original);
  const beforeDrag=await runtimeOf(page).evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width'));
  const box=await separator.boundingBox();await page.mouse.move(box.x+box.width/2,box.y+20);await page.mouse.down();await page.mouse.move(box.x-40,box.y+20);await page.keyboard.press('Escape');await page.mouse.up();
  assert.equal(await runtimeOf(page).evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width')),beforeDrag);
  await frame.locator('#sd-btn-back').click();
  await runtimeOf(page).waitForFunction(()=>document.activeElement.id==='search-input');
  report.steps.push({name:'OTC actual cached NAV in detail/table, no OHLC/minute/drawing; separator keyboard and cancelled drag; return source focus'});


}

export async function verifyNativeWatchManage(page,report) {
 const frame=page.frameLocator('iframe'),runtime=runtimeOf(page);
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));await frame.locator('#tab-watchlist').click();
 await frame.locator('#watchlist-create-group').click();await frame.locator('dialog[open] [name=name]').fill('DES isolated group');await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open]').waitFor({state:'detached'});
 const state=await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'));const source=state.groups.find(group=>group.name==='DES isolated group');assert.ok(source);
 await frame.locator('#watchlist-groups [data-id="'+source.id+'"]').waitFor();
 const open=async()=>{await frame.locator('#watchlist-manage-group').click();await frame.locator('dialog[open] [name=op]').waitFor();};
 const submit=async()=>{await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open]').waitFor({state:'detached'});};
 await open();await frame.locator('dialog[open] [name=name]').fill('DES renamed group');await submit();
 await runtime.evaluate(async id=>{await Fund.api('POST','/api/watchlists',{id:'des-target',name:'DES target'});await Fund.api('POST','/api/watchlists/items',{watchlistId:id,symbol:'sz000001',name:'Isolated stock',assetType:'stock'});await Fund.actions.refreshWatchlist();},source.id);
 await open();await frame.locator('dialog[open] [name=op]').selectOption('move');await frame.locator('dialog[open] [name=target]').selectOption('des-target');await frame.locator('dialog[open] [name=symbol]').selectOption('sz000001');await submit();
 const moved=await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'));assert.equal(moved.items.find(item=>item.symbol==='sz000001'&&item.watchlistId==='des-target').name,'Isolated stock');assert.ok(!moved.items.some(item=>item.symbol==='sz000001'&&item.watchlistId===source.id));
 await frame.locator('#watchlist-groups [data-id=des-target]').click();await runtime.waitForFunction(()=>Fund.state.watchlistItems.some(item=>item.symbol==='sz000001'));
 await open();await frame.locator('dialog[open] [name=op]').selectOption('orderItem');await frame.locator('dialog[open] [name=sortOrder]').fill('7');await submit();
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.find(item=>item.watchlistId==='des-target').sortOrder,7);
 await open();await frame.locator('dialog[open] [name=name]').fill('DES reviewed draft');
 await runtime.evaluate(()=>Fund.api('POST','/api/watchlists',{id:'des-concurrent',name:'Concurrent metadata'}));await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open] [data-review]:visible').waitFor();assert.equal(await frame.locator('dialog[open] [name=name]').inputValue(),'DES reviewed draft');
 await frame.locator('dialog[open] [data-review]').click();await frame.locator('dialog[open] [type=submit]:enabled').waitFor();await submit();
 await open();await frame.locator('dialog[open] [name=op]').selectOption('delete');await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open] [data-yes]').click();await frame.locator('dialog[open]').waitFor({state:'detached'});
 const deleted=await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'));assert.ok(!deleted.groups.some(group=>group.id==='des-target'));assert.ok(!deleted.items.some(item=>item.watchlistId==='des-target'));
 report.steps.push({name:'self-managed watchlist real Native: create/rename/atomic move/item order, concurrent digest conflict preserves draft and explicit review, confirmed group delete removes only watchlist metadata'});
 await frame.locator('#tab-home').click();
}
