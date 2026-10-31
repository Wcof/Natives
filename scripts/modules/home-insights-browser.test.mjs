// Real Chrome renderer, synthetic read-only snapshot; no user ledger.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {readFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {installHomeFixture} from './home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../../modules/fund/ui/src'),out=resolve(import.meta.dirname,'../../.tmp/home-insights');
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try {
 const page=await browser.newPage({viewport:{width:1280,height:900}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(`<style>${readFileSync(resolve(root,'../dist/styles/theme.css'),'utf8')}${readFileSync(resolve(root,'styles/home.css'),'utf8')}${readFileSync(resolve(root,'styles/investment.css'),'utf8')}</style>${readFileSync(resolve(root,'partials/view-home.html'),'utf8')}`);
 await page.evaluate(installHomeFixture);
 for(const file of ['core.js','home-i18n.js','home-trend-chart.js','home-overview.js','home.js','home-range.js','home-manage.js'])await page.addScriptTag({path:resolve(root,file)});
 await page.evaluate(async()=>{Fund.state.token='test';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);});
 assert.equal(await page.locator('#home-finance-details').count(),0,'finance tools removed');
 assert.equal(await page.locator('.home-opportunities-region').count(),0,'opportunities removed');
 assert.equal(await page.locator('#home-opportunity-body').count(),0,'opportunity body removed');
 assert.equal(await page.locator('#home-history-points').count(),0,'saved valuation points removed');

 // ADR-0049 D8：水位线与刻度必须共用同一纵向基准，水位线不得进入指标带，
 // 罐体必须从刻度槽右缘起（否则一改卡内边距就会把刻度推到卡外被 overflow 裁掉）。
 const axis=await page.evaluate(()=>{
  const scale=document.querySelector('.goal-gauge .goal-scale'),water=document.querySelector('.goal-water');
  const s=scale.getBoundingClientRect(),w=water.getBoundingClientRect();
  const tickBox=t=>{const r=t.getBoundingClientRect();return{label:t.textContent,cy:r.top+r.height/2};};
  return{top:s.top,height:s.height,scaleX:s.x,scaleW:s.width,level:Number(water.getAttribute('aria-valuenow')),
   lineTop:document.querySelector('.goal-water-line').getBoundingClientRect().top,waterX:w.x,
   metricTop:document.querySelector('.goal-water-metrics').getBoundingClientRect().top,
   ticks:[...scale.querySelectorAll('.goal-tick')].map(tickBox)};
 });
 assert.ok(axis.level>0,'fixture must expose a water level for the axis contract');
 assert.ok(Math.abs(axis.lineTop-(axis.top+(1-axis.level/100)*axis.height))<=1.5,`water line off axis: ${axis.lineTop} vs ${axis.top}+${axis.height}`);
 assert.ok(axis.lineTop<=axis.metricTop-12,'water line must stay above the metrics band');
 assert.ok(Math.abs(axis.waterX-(axis.scaleX+axis.scaleW))<=0.5,'tank must start exactly where the tick gutter ends');
 for(const tick of axis.ticks){
  const value=Number.parseInt(tick.label,10);
  assert.ok(Math.abs(tick.cy-(axis.top+(1-value/100)*axis.height))<=1,`tick ${tick.label} off axis: ${tick.cy}`);
 }
 // ADR-0049 D7-3：hero 默认 2 位小数，但 2 位会把非零水位抹成 0 时必须展开到 4 位（ppm 精度不丢）。
 await page.evaluate(async()=>{
  const snapshot=structuredClone(Fund.home.snapshot());
  snapshot.goals.progress={...snapshot.goals.progress,ratioPpm:10,ratioTenths:0,computable:true};
  window.snapshotOverride=snapshot;await Fund.home.refresh();
 });
 assert.equal(await page.locator('#home-goal-progress strong').textContent(),'0.001%','sub-tenth coverage must not collapse to 0.00%');
 assert.equal(await page.locator('.goal-water').getAttribute('aria-valuetext'),'0.001%');
 await page.evaluate(async()=>{window.snapshotOverride=null;await Fund.home.refresh();});

 assert.equal(await page.locator('#home-market-tabs [data-market-group]').count(),3,'A/HK/US market groups');
 assert.match(await page.locator('#home-market-body').innerText(),/3,000\.00/,'A-share quote');
 await page.locator('#home-market-tabs [data-market-group=hk]').click();
 assert.match(await page.locator('#home-market-body').innerText(),/恒生指数/,'HK group');
 await page.locator('#home-market-tabs [data-market-group=us]').click();
 assert.match(await page.locator('#home-market-body').innerText(),/标普500/,'US group');
 await page.evaluate(async()=>{
  const snapshot=structuredClone(Fund.home.snapshot());
  const today=snapshot.requestedRange.today, back=n=>new Date(Date.parse(today+'T00:00:00Z')-n*86400000).toISOString().slice(0,10);
  // 类型按钮只对「有可绘制走势」的类型可点：给分类两个日估值，再验证选中态。
  snapshot.portfolio.history.points=[
   {date:back(2),valueMinor:'12345678',quality:'ready',groups:[{groupId:'g1',valueMinor:'20000'}]},
   {date:back(1),valueMinor:'12400000',quality:'ready',groups:[{groupId:'g1',valueMinor:'25000'}]},
  ];
  snapshot.groupAllocation.groups[0].currentMinor='25000';
  window.snapshotOverride=snapshot;await Fund.home.refresh();Fund.home.applyDisplayMode('amount',false);
 });
 assert.equal(await page.locator('#home-history-categories [data-history-cat]').count(),2,'top row lists 全部资产 plus the category; the other one has no trend');
 await page.locator('#home-history-categories [data-history-cat="g1"]').click();
 assert.equal(await page.locator('#home-history-chart [data-series="total"]').count(),0,'selecting a category excludes the unrelated total from the chart');
 assert.equal(await page.locator('#home-history-chart [data-series="g1"]').count(),1,'the selected category still has its own real series');
 await page.locator('#home-history-chart .history-plot').press('End');
 assert.match(await page.locator('#home-history-chart .history-tip').innerText(),/250\.00/);
 assert.doesNotMatch(await page.locator('#home-history-chart .history-tip').innerText(),/124,000|123,456/,'tooltip excludes total assets in category view');
 assert.match(await page.locator('#home-history-selection').innerText(),/Equity Group/);
 await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
 const masked=await page.locator('#home-history-categories [data-history-cat]').first().innerText();
 assert.match(masked,/^\*{4}/,'privacy mode masks category names');
 assert.doesNotMatch(masked,/Equity|\d{3}/,'privacy mode masks values and percentages');
 // 期间净收益大数字已并入三指标汇总（¥ 前缀无符号），leading-plus 断言随之移除。
 assert.deepEqual(errors,[]);
 await page.close();
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
  const matrix=await browser.newPage({viewport:{width,height:1000}}),matrixErrors=[];matrix.on('pageerror',e=>matrixErrors.push(e.message));
  const css=['theme','layout','panel','home','investment'].map(name=>readFileSync(resolve(root,'../dist',`styles/${name}.css`),'utf8')).join('\n');
  await matrix.setContent(`<style>${css}</style>${readFileSync(resolve(root,'../dist/partials/view-home.html'),'utf8')}`);
  await matrix.evaluate(installHomeFixture);
  for(const file of ['core.js','home-i18n.js','home-trend-chart.js','home-overview.js','home.js','home-range.js','home-manage.js'])await matrix.addScriptTag({path:resolve(root,'../dist',file)});
  await matrix.evaluate(async({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.applyLanguage(language);Fund.state.token='test';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);},{theme,language});
  const merged=matrix.locator('.home-performance-card');
  assert.equal(await merged.count(),1);
  assert.equal(await merged.locator('.home-income-card').count(),4);
  assert.equal(await merged.locator('#home-history-chart,#home-market-body,#home-benchmark-chart,#home-attribution-body').count(),4);
  assert.equal(await matrix.locator('.home-overview-region > .home-income-grid').count(),0,'summary belongs to the merged card');
  assert.equal(await merged.locator(':scope > header h2').innerText(),language==='en'?'Returns and performance':'收益与表现');
  assert.equal(await matrix.evaluate(()=>{
   const card=document.querySelector('.home-performance-card'),allocation=document.querySelector('.home-allocation-card');
   return card.nextElementSibling===allocation&&card.getBoundingClientRect().bottom<=allocation.getBoundingClientRect().top;
  }),true,'the whole merged card sits immediately above allocation');
  assert.equal(await matrix.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true,'no horizontal overflow');
  assert.equal(await merged.evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
  // 2026-10-25 布局精简：三指标汇总、累计收益、已投资市值与检查条都已并入别处。
  assert.equal(await merged.locator('#hc-cumulative, #hc-invested, #home-trend-summary, #home-trend-inspection').count(),0,'redundant summary blocks are gone');
  assert.equal(await merged.locator('#home-all-daily-value, #home-all-period-value').count(),2,'收益卡仍保留当日与区间收益');
  assert.equal(await merged.locator('#home-market-tabs [data-market-group]').count(),3);
  assert.equal(await merged.locator('#home-history-categories [data-history-cat]').count(),1,'base fixture has no category trend, so only 全部资产 is offered');
  assert.equal(await merged.locator('#home-history-categories [data-history-cat="total"]').count(),1,'全部资产 is always available');
  await merged.locator('[data-range=today]').focus();await matrix.keyboard.press('Enter');
  await matrix.waitForFunction(()=>Fund.home.snapshot()?.requestedRange?.key==='today');
  assert.equal(await merged.locator('[data-range=today]').getAttribute('aria-pressed'),'true');
  await merged.locator('#home-market-tabs [data-market-group=hk]').focus();await matrix.keyboard.press('Enter');
  assert.equal(await merged.locator('[data-market-group=hk]').getAttribute('aria-pressed'),'true');
  await matrix.screenshot({path:resolve(out,`merged-${theme}-${language}-${width}.png`),fullPage:true});
  await matrix.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
  assert.equal(await merged.locator('#home-history-chart svg').count(),0);
  assert.equal(await merged.locator('#home-stock-value').innerText(),'****');
  assert.deepEqual(matrixErrors,[]);await matrix.close();
 }
 console.log('Home insights: merged summary/history card above allocation; 8 Chrome layouts, category-only chart and tooltip, market/range keyboard controls and privacy passed.');
} finally { await browser.close(); }
