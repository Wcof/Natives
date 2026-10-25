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
 for(const file of ['core.js','home-i18n.js','home-overview.js','home.js','home-range.js','home-manage.js'])await page.addScriptTag({path:resolve(root,file)});
 await page.evaluate(async()=>{Fund.state.token='test';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);});
 assert.equal(await page.locator('#home-finance-details').count(),0,'finance tools removed');
 assert.equal(await page.locator('.home-opportunities-region').count(),0,'opportunities removed');
 assert.equal(await page.locator('#home-opportunity-body').count(),0,'opportunity body removed');
 assert.equal(await page.locator('#home-history-points').count(),0,'saved valuation points removed');

 assert.equal(await page.locator('#home-market-tabs [data-market-group]').count(),3,'A/HK/US market groups');
 assert.match(await page.locator('#home-market-body').innerText(),/3,000\.00/,'A-share quote');
 await page.locator('#home-market-tabs [data-market-group=hk]').click();
 assert.match(await page.locator('#home-market-body').innerText(),/恒生指数/,'HK group');
 await page.locator('#home-market-tabs [data-market-group=us]').click();
 assert.match(await page.locator('#home-market-body').innerText(),/标普500/,'US group');
 await page.evaluate(async()=>{
  const snapshot=structuredClone(Fund.home.snapshot());
  snapshot.portfolio.history.points=[{date:snapshot.requestedRange.today,valueMinor:'12345678',quality:'reference'}];
  snapshot.groupAllocation.groups[0].currentMinor='25000';
  window.snapshotOverride=snapshot;await Fund.home.refresh();Fund.home.applyDisplayMode('amount',false);
 });
 assert.equal(await page.locator('#home-history-series [data-history-group]').count(),3);
 await page.locator('[data-history-group=g1]').click();
 assert.equal(await page.locator('.history-single strong').innerText(),'123,456.78','total assets stay the headline even with a category selected');
 assert.match(await page.locator('#home-history-selection').innerText(),/Equity Group/);
 await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
 assert.equal(await page.locator('#home-history-series [data-history-group]').first().innerText(),'****');
 assert.doesNotMatch(await page.locator('#hc-return-value').innerText(),/^\+/, 'money amount has no leading plus');
 assert.deepEqual(errors,[]);
 await page.close();
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
  const matrix=await browser.newPage({viewport:{width,height:1000}}),matrixErrors=[];matrix.on('pageerror',e=>matrixErrors.push(e.message));
  const css=['theme','layout','panel','home','investment'].map(name=>readFileSync(resolve(root,'../dist',`styles/${name}.css`),'utf8')).join('\n');
  await matrix.setContent(`<style>${css}</style>${readFileSync(resolve(root,'../dist/partials/view-home.html'),'utf8')}`);
  await matrix.evaluate(installHomeFixture);
  for(const file of ['core.js','home-i18n.js','home-overview.js','home.js','home-range.js','home-manage.js'])await matrix.addScriptTag({path:resolve(root,'../dist',file)});
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
  assert.equal(await merged.locator('#hc-invested').evaluate(el=>{
   const parent=el.getBoundingClientRect(),button=el.querySelector('.hc-detail').getBoundingClientRect();
   return button.top>=parent.top&&button.bottom<=parent.bottom;
  }),true,'invested detail stays with its own metric instead of overlapping the merged heading');
  assert.equal(await merged.locator('#home-market-tabs [data-market-group]').count(),3);
  assert.equal(await merged.locator('#home-history-series [data-history-group]').count(),3);
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
 console.log('Home insights: merged summary/history card above allocation; 8 Chrome layouts, total headline with category overlay, market/range keyboard controls and privacy passed.');
} finally { await browser.close(); }
