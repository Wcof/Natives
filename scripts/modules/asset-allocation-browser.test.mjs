// Production UI, synthetic transport; real ledger coverage is recorded separately.
import assert from 'node:assert/strict';
import {readFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
import {installHomeFixture} from './home-browser-fixture.mjs';

const root=resolve(import.meta.dirname,'../../modules/fund/ui/dist');
const out=resolve(import.meta.dirname,'../../.tmp/asset-allocation-merge');
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try{
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
  const page=await browser.newPage({viewport:{width,height:950}}),errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  const css=['theme','layout','panel','home','investment'].map(name=>readFileSync(resolve(root,`styles/${name}.css`),'utf8')).join('\n');
  await page.setContent(`<style>${css}</style>${readFileSync(resolve(root,'partials/view-home.html'),'utf8')}`);
  await page.evaluate(installHomeFixture);
  for(const file of ['core','home-i18n','home-overview','home-opportunities','home','home-range','home-manage'])await page.addScriptTag({path:resolve(root,file+'.js')});
  await page.evaluate(({theme,language})=>{
   document.documentElement.dataset.theme=theme;Fund.applyLanguage(language);Fund.home.applyDisplayMode('amount',false);
   window.allocation={configured:true,groups:[
    {groupId:'stock',name:'Equity <script>safe</script>',description:'Core investments',ratioTenths:600,currentTenths:500,currentMinor:'6172839',assets:[{name:'Asset one',code:'600000',instrumentKind:'stock',valueMinor:'6172839'}]},
    {groupId:'cash',name:'Cash Group',description:'Reserve',ratioTenths:400,currentTenths:500,currentMinor:'6172839',assets:[]}
   ],unassignedMinor:'0',unallocatedTenths:0,invalidRefs:[]};
   Fund.home.renderGroupAllocation(allocation);
  },{theme,language});
  const card=page.locator('.home-allocation-card');
  assert.equal(await card.count(),1);assert.equal(await page.locator('.home-distribution-card,.home-combination-card').count(),0);
  assert.equal(await card.locator('#home-allocation-total').innerText(),'123,456.78');
  assert.equal(await card.locator('#home-allocation-target').innerText(),'100.0%');
  assert.equal(await card.locator('.combination-table tbody tr').count(),2);
  assert.deepEqual(await card.locator('.combination-deviation').allTextContents(),['-10.0pp','+10.0pp']);
  assert.deepEqual(await card.locator('.combination-track > span').evaluateAll(nodes=>nodes.map(node=>node.style.width)),['50%','50%'],'fill represents actual weight, independently of target weight');
  assert.equal(await card.locator('.combination-track > i').count(),0,'no ambiguous second marker');
  for(const id of ['home-distribution-body','home-combination-body'])assert.ok(await card.locator('#'+id).evaluate(el=>parseFloat(getComputedStyle(el).paddingRight)>=12),'content keeps a gap before the scrollbar');
  assert.equal(await card.locator('script').count(),0);
  assert.doesNotMatch(await card.innerText(),/fundHome[A-Z]/);
  const summary=card.locator('details > summary').first();await summary.focus();await page.keyboard.press('Enter');
  await page.evaluate(()=>Fund.home.renderGroupAllocation(allocation));
  assert.equal(await card.locator('details').first().evaluate(el=>el.open),true);
  assert.equal(await summary.evaluate(el=>el===document.activeElement),true);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  assert.equal(await card.evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
  assert.equal(await card.locator('.combination-table').evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
  await card.screenshot({path:resolve(out,`${theme}-${language}-${width}.png`)});
  for(const mode of ['percent','hidden']){
   await page.evaluate(mode=>{Fund.home.applyDisplayMode(mode,false);Fund.home.renderGroupAllocation(allocation);},mode);
   assert.doesNotMatch(await card.innerText(),/123,456|61,728/);
   if(mode==='hidden'){
    assert.doesNotMatch(await card.innerText(),/Equity|Cash Group|50\.0|60\.0|100\.0/);
    assert.equal(await card.locator('.home-allocation-bar,.combination-track').count(),0);
   }
  }
  await page.evaluate(()=>{
   Fund.home.applyDisplayMode('amount',false);
   const missing=structuredClone(allocation);missing.groups[0].currentMinor=null;missing.groups[0].currentTenths=null;missing.unassignedMinor=null;
   Fund.home.renderGroupAllocation(missing);
  });
  assert.equal(await card.locator('#home-allocation-total').innerText(),'–');
  assert.equal(await card.locator('.home-allocation-bar').count(),0);
  assert.equal(await card.locator('.combination-deviation').first().innerText(),'–');
  assert.equal(await card.locator('.combination-row').first().locator('.combination-track').count(),0,'unknown weight has no invented empty progress');
  await page.evaluate(()=>{const bounds=structuredClone(allocation);bounds.groups[0].currentTenths=0;bounds.groups[1].currentTenths=1000;Fund.home.renderGroupAllocation(bounds);});
  assert.deepEqual(await card.locator('.combination-track > span').evaluateAll(nodes=>nodes.map(node=>node.style.width)),['0%','100%']);
  await page.evaluate(()=>Fund.home.renderGroupAllocation({configured:false}));
  assert.equal(await card.locator('#home-allocation-target').innerText(),'–');
  await page.evaluate(()=>{const many=structuredClone(allocation);many.groups=Array.from({length:8},(_,i)=>({...many.groups[0],groupId:String(i),name:'Group '+i,currentTenths:125,ratioTenths:125}));Fund.home.renderGroupAllocation(many);});
  assert.equal(await card.locator('.home-allocation-legend li').count(),6);
  assert.equal(await card.locator('details').count(),8);
  await page.addStyleTag({content:'#home-combination-body::-webkit-scrollbar{width:16px}'});
  assert.equal(await card.locator('#home-combination-body').evaluate(el=>{
   const table=el.querySelector('table'),bounds=el.getBoundingClientRect();
   return table.getBoundingClientRect().right<=bounds.left+el.clientWidth-12+1;
  }),true,'actual table ends before the scrollbar and its 12 px content gap');
  await card.locator('#home-distribution-body').evaluate(el=>{el.scrollTop=100;});
  const scroll=await card.locator('#home-distribution-body').evaluate(el=>el.scrollTop);
  await page.evaluate(()=>document.dispatchEvent(new Event('fund:display-mode-changed')));
  // An identical projection preserves the user's scroll, independently of theme/layout.
  await page.evaluate(()=>{const many=structuredClone(allocation);many.groups=Array.from({length:8},(_,i)=>({...many.groups[0],groupId:String(i),name:'Group '+i,currentTenths:125,ratioTenths:125}));Fund.home.renderGroupAllocation(many);});
  assert.equal(await card.locator('#home-distribution-body').evaluate(el=>el.scrollTop),scroll);
  assert.deepEqual(errors,[]);await page.close();
 }
 console.log('Asset allocation: 8 Chrome layouts, merged structure, exact totals, privacy, unknown/empty, escaping, 6-series limit, expand/focus and scroll passed.');
}finally{await browser.close();}
