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
  await page.locator('.home-history > summary').click();
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
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  }
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('Home history: single point, partial current assets, historical empty, privacy, valuation entry/cancel, themes/languages/narrow passed');
}finally{await browser.close();}
