// Production information page; synthetic transport only, no user database.
import assert from 'node:assert/strict';
import {readFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
import {installHomeFixture} from './home-browser-fixture.mjs';
import {shellCss} from '../des/shell-harness.mjs';
const root=resolve(import.meta.dirname,'../../modules/fund/ui/src');
const browser=await chromium.launch({channel:'chrome',headless:true});
try{
 const page=await browser.newPage();
 await page.setContent(readFileSync(resolve(root,'partials/view-home.html'),'utf8'));
 await page.addStyleTag({content:shellCss()});
 await page.evaluate(()=>{const button=document.createElement('button');button.id='home-info-next';button.hidden=true;document.getElementById('home-evidence-body').appendChild(button);});
 await page.evaluate(installHomeFixture);
 await page.evaluate(()=>{
  const original=fetch;window.informationCalls=[];window.newsReady=false;window.sourceAttempt=null;
  const response=data=>new Response(JSON.stringify(data),{headers:{'Content-Type':'application/json'}});
  window.fetch=async(url,options={})=>{
   const path=String(url);informationCalls.push({path,method:options.method||'GET',body:options.body});
   if(path.includes('/api/information/sources'))return response({sources:['em_stock_news','em_stock_ann','em_fund_ann','em_finance'].map(sourceId=>({sourceId,enabled:!window.sourcesDisabled,lastAttemptAt:sourceAttempt,lastSuccessAt:sourceAttempt,lastErrorCode:null}))});
   if(path.includes('/api/information/items'))return response({items:newsReady?[{id:1,title:'Collected source article',summary:'Real transport shape',sourceId:'em_stock_news',publishedAt:new Date().toISOString(),completeness:'excerpt'}]:[],assetOptions:[],groupOptions:[]});
   if(path.includes('/api/information/refresh')){
    if(window.failInformation)return new Response(JSON.stringify({error:'INFORMATION_UPSTREAM'}),{status:503});
    if(window.deferInformation)return response({taskId:9});
    sourceAttempt=new Date().toISOString();newsReady=true;
    return response({result:{outcome:{sources:[{source_id:'em_stock_news',status:'ok_with_items',count:1}],covered_identities:window.nextOffset?50:1,total_identities:window.nextOffset?60:1,next_identity_offset:window.nextOffset||null}}});
   }
   if(path.includes('/api/information/cancel'))return response({cancelled:true});
   if(path.includes('/api/information/status')&&!window.deferInformation)return response({status:'complete',result:{outcome:{}}});
   if(path.includes('/api/information/status'))return new Promise(resolve=>{window.releaseInformation=()=>resolve(response({status:'complete',result:{outcome:{}}}));});
   return original(url,options);
  };
 });
 for(const file of ['core.js','home-i18n.js','home.js','home-manage.js'])await page.addScriptTag({path:resolve(root,file)});
 await page.evaluate(async()=>{Fund.state.token='fixture';Fund.home.applyDisplayMode('amount',false);document.getElementById('view-home').hidden=true;document.getElementById('view-news').hidden=false;await Fund.actions.loadNewsPage();});
 const collects=()=>page.evaluate(()=>informationCalls.filter(call=>call.path.includes('/api/information/refresh')).length);
 assert.equal(await collects(),1,'opening an uncollected news page must collect, rather than only reading an empty cache');
 assert.equal(await page.evaluate(()=>JSON.parse(informationCalls.find(call=>call.path.includes('/api/information/refresh')).body).scope),'all','news entry collects holdings and watchlist together');
 assert.match(await page.locator('#news-page-body').innerText(),/Collected source article/);
 assert.equal(await page.locator('#news-page-body details.news-advanced').count(),1,'advanced filters must not replace the Demo news reading layout');
 assert.equal(await page.locator('#news-page-body details.news-advanced').getAttribute('open'),null);
 await page.evaluate(()=>Fund.actions.loadNewsPage());
 assert.equal(await collects(),1,'fresh source results are reused');
 await page.locator('#news-page-refresh').click();
 await page.waitForFunction(()=>informationCalls.filter(call=>call.path.includes('/api/information/refresh')).length===2);
 assert.match(await page.locator('#news-page-status').innerText(),/\S/);
 await page.evaluate(()=>{window.nextOffset=50;});
 await page.locator('#news-page-refresh').click();
 await page.locator('#news-page-next').waitFor({state:'visible'});
 assert.equal(await page.locator('#home-info-next').getAttribute('hidden'),'','a combined collection cursor must not be reused for the home holdings-only scope');
 await page.evaluate(()=>{window.failInformation=true;});
 await page.locator('#news-page-refresh').click();
 await page.locator('#news-page-status').filter({hasText:/失败|could not|unavailable/i}).waitFor();
 assert.equal(await page.locator('#news-page-refresh').isEnabled(),true);
 assert.match(await page.locator('#news-page-body').innerText(),/Collected source article/,'failed refresh keeps the last successful data');
 await page.evaluate(()=>{window.failInformation=false;window.deferInformation=true;});
 await page.locator('#news-page-refresh').click();
 await page.locator('#news-page-refresh').filter({hasText:/取消|Cancel/}).waitFor();
 await page.waitForFunction(()=>!!window.releaseInformation);
 await page.locator('#news-page-refresh').click();
 assert.ok(await page.evaluate(()=>informationCalls.some(call=>call.path.includes('/api/information/cancel'))));
 assert.match(await page.locator('#news-page-status').innerText(),/取消|cancel/i);
 await page.evaluate(()=>{releaseInformation();window.deferInformation=false;});
 await page.evaluate(async()=>{window.sourcesDisabled=true;window.sourceAttempt=null;await Fund.actions.loadNewsPage();});
 assert.equal(await collects(),5,'disabled sources must not be silently enabled');
 await page.evaluate(async()=>{window.sourcesDisabled=false;Fund.home.applyDisplayMode('hidden',false);await Fund.actions.loadNewsPage();});
 assert.equal(await collects(),5,'hidden private scope must not collect');
 assert.equal(await page.locator('#news-page-next').getAttribute('hidden'),'');
 await page.evaluate(async()=>{Fund.home.applyDisplayMode('amount',false);await Fund.actions.loadNewsPage(false);});
 await page.locator('[data-info-scope="market"]').press('Enter');
 await page.locator('[data-info-scope="market"][aria-pressed="true"]').waitFor();
 assert.equal(await page.evaluate(()=>document.activeElement.dataset.infoScope),'market','scope changes retain keyboard focus');
 await page.locator('#news-page-refresh').click();
 await page.waitForFunction(()=>JSON.parse(informationCalls.filter(call=>call.path.includes('/api/information/refresh')).at(-1).body).scope==='market');
 await page.locator('#news-page-refresh').filter({hasText:/刷新|Refresh/}).waitFor();
 await page.locator('.news-advanced summary').press('Enter');
 assert.equal(await page.locator('.news-advanced').getAttribute('open'),'');
 await page.locator('[name=q]').fill('policy');
 await page.locator('[name=q]').press('Enter');
 await page.waitForFunction(()=>informationCalls.some(call=>call.path.includes('q=policy')));
 await page.evaluate(async()=>{
  let starts=0,checks=0,finished=false;
  const original=fetch;
  window.fetch=async(url,options)=>{
   const path=String(url),response=data=>new Response(JSON.stringify(data));
   if(path.endsWith('/api/information/refresh')){
    starts++;
    if(starts===1)return response({taskId:101});
    if(!finished)throw Error('new collection started before the cancelled worker finished');
    return response({result:{outcome:{sources:[]}}});
   }
   if(path.includes('/api/information/status?taskId=101')){
    checks++;finished=checks>=2;
    return response({taskId:101,status:'cancelled',result:finished?{cancelled:true}:null});
   }
   if(path.endsWith('/api/information/cancel'))return response({cancelled:true});
   return original(url,options);
  };
  const first=Fund.home.requestInformation('/api/information/refresh',{scope:'all'});
  await new Promise(resolve=>setTimeout(resolve,20));
  Fund.home.cancelInformationRequest();
  const second=await Fund.home.requestInformation('/api/information/refresh',{scope:'market'});
  if(await first!==null||starts!==2||!finished||!second.outcome)throw Error('cancel/drain/restart ownership failed');
  window.fetch=original;
 });
 await page.addScriptTag({path:resolve(root,'home-evidence.js')});
 await page.evaluate(async()=>{
  await Fund.home.readSnapshot();
  window.currentSnapshot=Fund.home.snapshot;
  Fund.home.snapshot=()=>({...currentSnapshot(),dataRevision:'stale-before-news-refresh'});
  Fund.home.selectNewsForAI({id:1,title:'Collected source article',sourceId:'em_finance'});
 });
 await page.locator('[data-act=home-ai-confirm]:enabled').waitFor({state:'attached'});
 assert.notEqual(await page.evaluate(()=>lastEvidenceBody.dataRevision),'stale-before-news-refresh','a freshly collected article must use a current ledger snapshot for evidence');
 await page.evaluate(()=>{Fund.home.snapshot=currentSnapshot;Fund.home.cancelAI();});
 const out=resolve(root,'../../../../.tmp/information-layout');mkdirSync(out,{recursive:true});
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1440,320]){
  await page.setViewportSize({width,height:1000});
  await page.evaluate(async({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.applyLanguage(language);await Fund.actions.loadNewsPage(false);},{theme,language});
  await page.locator('.news-articles .info-item').waitFor();
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  await page.locator('.news-advanced').evaluate(node=>node.open=false);
  await page.locator('#view-news').screenshot({path:resolve(out,`news-${theme}-${language}-${width}.png`)});
 }
 console.log('Information page: first collection, fresh cache, manual refresh, visible failure, disabled sources and privacy passed');
}finally{await browser.close();}
