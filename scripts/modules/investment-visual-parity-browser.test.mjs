// Reference geometry and real production renderers; synthetic ledger, no user DB.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
import {shellHtml} from '../des/shell-harness.mjs';
import {installHomeFixture} from './home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../..'),out=resolve(root,'.tmp/investment-visual-parity');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const results=[];
try{
 for(const theme of ['archive','volt'])for(const width of [1440,768,320]){
  const page=await browser.newPage({viewport:{width,height:1000}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(shellHtml());await page.evaluate(installHomeFixture);
  for(const file of ['core.js','home-i18n.js','home-overview.js','home-opportunities.js','home.js','home-range.js','home-manage.js','home-evidence.js'])await page.addScriptTag({path:resolve(root,'modules/fund/ui/src',file)});
  await page.evaluate(async theme=>{document.documentElement.dataset.theme=theme;document.querySelectorAll('.main-column > section').forEach(s=>s.hidden=true);document.getElementById('view-home').hidden=false;Fund.applyI18nDom();Fund.state.token='synthetic';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);},theme);
  const facts=await page.evaluate(()=>{
   const box=s=>{const r=document.querySelector(s).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};};
   const order=['.home-ledger-strip','.home-goal-water','.home-allocation-grid','.home-decision-grid','.home-income-grid','.home-history'];
   return {width:innerWidth,wrap:box('.home-wrap'),hero:box('.home-ledger-strip'),water:box('.home-goal-water'),nav:box('nav.main-nav'),brand:box('.brand-group'),cards:[...document.querySelectorAll('.home-ledger-strip > .home-ledger-cell')].length,order:order.every((s,i)=>!i||!!(document.querySelector(order[i-1]).compareDocumentPosition(document.querySelector(s))&Node.DOCUMENT_POSITION_FOLLOWING)),radius:getComputedStyle(document.querySelector('.home-news-panel')).borderRadius,font:getComputedStyle(document.body).fontSize,historyOpen:document.querySelector('.home-history').open,overflow:document.documentElement.scrollWidth>innerWidth+1};
  });
  assert.equal(facts.order,true);assert.equal(facts.cards,4);assert.equal(facts.font,'14px');assert.equal(facts.radius,'14px');assert.equal(facts.historyOpen,false);assert.equal(facts.overflow,false);
  assert.ok(Math.abs(facts.hero.width-facts.water.width)<1);assert.ok(facts.water.y>=facts.hero.y+facts.hero.height);
  if(width===1440){assert.equal(facts.wrap.width,1280);assert.ok(Math.abs(facts.nav.y-facts.brand.y)<25,'wide navigation shares the brand row');}
  await page.locator('.home-history > summary').press('Enter');assert.equal(await page.locator('.home-history').evaluate(n=>n.open),true);
  await page.locator('.home-extra-research > summary').click();assert.equal(await page.locator('.home-extra-research').evaluate(n=>n.open),true);
  await page.locator('.home-history > summary').press('Enter');await page.locator('.home-extra-research > summary').click();
  await page.locator('#home-group-manage').click();await page.locator('dialog[open]').waitFor();await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>document.activeElement.id),'home-group-manage');
  await page.screenshot({path:resolve(out,`production-home-${theme}-${width}.png`)});
  assert.deepEqual(errors,[]);results.push({theme,width,facts,errors});await page.close();
 }
 writeFileSync(resolve(out,'geometry-and-interactions.json'),JSON.stringify(results,null,2));console.log('Prototype geometry: 6 theme/width cases, headline, full-width water, exact order, folded sections, dialog/focus passed');
}finally{await browser.close();}
