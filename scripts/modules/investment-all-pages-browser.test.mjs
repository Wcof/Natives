// Production UI, isolated synthetic transport. Native ledger coverage runs separately.
import assert from 'node:assert/strict';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
import {shellHtml,shellScripts} from '../des/shell-harness.mjs';
import {installHomeFixture} from './home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../..'),ui=resolve(root,'modules/fund/ui/src'),out=resolve(root,'.tmp/investment-all-pages');
mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const results=[];
try{
 const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent(shellHtml());await page.evaluate(installHomeFixture);
 await page.evaluate(()=>{
  const original=fetch,date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  const json=(body,status=200)=>new Response(JSON.stringify(body),{status,headers:{'Content-Type':'application/json'}});
  window.allPageCalls=[];
  window.allPageHolding={accountId:1,fundId:1,fundCode:'600519',fundName:'Synthetic equity',account:'Fixture account',instrumentKind:'stock',market:'SSE',venue:'exchange',quoteSymbol:'sh600519',quantity:'100',cost:'9000',averageCost:'90',marketValue:'10000',asOf:date,holdingDays:7,costUnknown:false,dataRevision:'7',income:{holding:{amount:'1000',rateBps:1111},day:{date,amount:'10'},yesterday:{date,amount:'-5'}}};
  window.allPageFund={...allPageHolding,fundId:2,fundCode:'000001',fundName:'Synthetic fund',instrumentKind:'fund',market:'CN',venue:'otc',quoteSymbol:null,quantity:'100',averageCost:'1',cost:'100',marketValue:'120',nav:{date,unitNav:'1.2'}};
  window.fetch=async(url,options={})=>{
   const path=new URL(String(url),'http://fixture').pathname;allPageCalls.push(path);
   if(window.failAllPageNews&&path==='/api/information/items')return json({error:'FIN_DB'},500);
   if(path==='/api/offline/assets')return json({assets:[{id:1,version:1,name:'Fixture gold',location:'Fixture safe',productType:'gold',valueMinor:'4200000',valuedAt:date,attrs:{weight:'50'},note:'Valuation checked',deleted:false},{id:2,version:1,name:'Fixture deposit',location:'Fixture bank',productType:'deposit',valueMinor:'6300000',valuedAt:date,attrs:{maturityDate:date},deleted:false}]});
   if(path==='/api/legacy-assets')return json({assets:[]});
   if(path==='/api/positions/detail'&&window.holdAllPageLedger){window.holdAllPageLedger=false;window.delayedLedgerSignal=options.signal;await new Promise(resolve=>{window.releaseAllPageLedger=resolve;});}
   if(path==='/api/positions/detail')return json({accounts:[{id:1,name:'Fixture account'}],holdings:[allPageHolding,allPageFund],dataRevision:'7'});
   if(path==='/api/holding/records')return json({opening:null,transactions:[],page:1,hasMore:false,dataRevision:'7'});
   if(path==='/api/transactions')return json({transactions:[{id:1,account:'Fixture account',fundCode:'600519',quantity:'10',price:'100',amount:'1000',fee:'1',tradeDate:date,type:'BUY',state:'confirmed'}]});
   if(path==='/api/watchlists')return json({digest:'fixture',groups:[{id:'default',name:'Default'},{id:'research',name:'Research'}],items:[{symbol:'sh600519',name:'Fixture equity',watchlistId:'research',assetType:'stock'}]});
   if(path==='/api/market/quotes')return json({quotes:[{code:'sh600519',name:'Fixture equity',price:100,changePct:1,change:1,asOf:date}]});
   if(path==='/api/market/sectors')return json({sectors:[{code:'BK01',name:'Fixture sector',changePct:1.2,changePct5:2,changePct20:3,topStockName:'Fixture equity',asOf:date}]});
   if(path==='/api/market/sector/stocks')return json({quotes:[{code:'sh600519',name:'Fixture equity',price:100,changePct:1}]});
   if(path==='/api/information/items')return json({items:[{id:1,title:'Fixture announcement',summary:'Synthetic public evidence',sourceId:'em_stock_ann',publishedAt:date+'T10:00:00Z',completeness:'excerpt'}],assetOptions:[],groupOptions:[]});
   if(path==='/api/information/decisions')return json({records:[{id:1,informationId:1,reportId:1,title:'Fixture announcement',createdAt:String(Math.floor(Date.now()/1000)),action:'observe',note:'Wait for evidence',snapshotId:'fixture',modelName:'Fixture model'}]});
   if(path==='/api/decisions')return json({records:[]});
   if(path==='/api/events')return json({events:[]});
   if(path==='/api/information/sources')return json({sources:[{sourceId:'em_stock_ann',enabled:true,lastSuccessAt:date}]});
   if(path==='/api/notify/channels')return json({channels:[]});
   if(path==='/api/notify/settings')return json({version:1,silenceStart:'22:00',silenceEnd:'08:00',retryMax:1,backoffBase:1,dedupMinutes:10});
   if(path==='/api/settings')return json({version:1,colorMode:'cn',linkPlatform:'eastmoney',sourcePriority:'em_stock_ann',benchmarkSymbol:'sh000300',quoteRefreshSeconds:10});
   if(path==='/api/settings/health')return json({databaseOk:true,schemaVersion:28,dataRevision:'7'});
   if(path==='/api/opportunities/stats')return json({feedback:{total:0,usefulRatePpm:null}});
   if(path==='/api/nav'||path==='/api/fund/nav-history')return json({nav:[{fundCode:'000001',navDate:date,unitNav:'1.2',source:'fixture',fetchedAt:Math.floor(Date.now()/1000)}]});
   if(path==='/api/fund/quarterly-positions')return json({report_date:date,stocks:[],bonds:[]});
   if(path==='/api/fund/classification')return json({code:new URL(String(url),'http://fixture').searchParams.get('code'),status:'ready',themes:[{id:'BK000054',name:'Fixture theme'}],sourceUrl:'https://fund.eastmoney.com/000001.html',fetchedAt:Date.now()/1000});
   if(path==='/api/holding/income-history')return json({rows:[{date,amount:'10'}],status:'ready',dataRevision:'7'});
   return original(url,options);
  };
 });
 await page.addScriptTag({path:resolve(ui,'echarts.min.js')});
 for(const file of shellScripts())await page.addScriptTag({path:resolve(ui,file)});
 await page.evaluate(async()=>{
  Fund.home.requestCapability=async op=>op==='options'?{models:[],defaultModel:null}:{permission:'granted'};
  Fund.state.token='synthetic';await Fund.actions.loadHome();Fund.home.applyDisplayMode('amount',false);
 });
 await page.evaluate(()=>{holdAllPageLedger=true;window.filteredLedger=Fund.actions.loadPositions();});
 await page.waitForFunction(()=>!!window.releaseAllPageLedger);
 for(const kind of ['otc','stock','all'])await page.evaluate(kind=>document.getElementById('pos-tab-'+kind).click(),kind);
 assert.equal(await page.evaluate(()=>delayedLedgerSignal.aborted),false,'category filters must preserve an in-flight common ledger request');
 await page.evaluate(async()=>{releaseAllPageLedger();await filteredLedger;});
 assert.equal(await page.locator('#holding-add').isEnabled(),true,'the shared ledger request releases the loading state after filter changes');
 async function navigate(view){
  if(view==='settings'){await page.evaluate(()=>Fund.actions.openFinanceSettings());return;}
  if(view==='stock-detail'){await page.evaluate(()=>Fund.actions.openStockDetail('sh600519','Synthetic equity','stock',allPageHolding));return;}
  if(view==='fund-detail'){await page.evaluate(()=>Fund.openFundDetail(allPageFund));return;}
  const button=page.locator('#tab-'+view);if(!await button.isVisible())await page.locator('#btn-tools-menu').click();await button.click();
  if(view==='assets')await page.evaluate(()=>Fund.actions.loadAssets());
  if(view==='tx')await page.evaluate(()=>Fund.actions.loadTx());
  if(view==='news')await page.evaluate(()=>Fund.actions.loadNewsPage());
  if(view==='history')await page.evaluate(()=>Fund.actions.loadDecisionHistory());
 }
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1440,320]){
  await page.setViewportSize({width,height:1000});
  await page.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.applyLanguage(language);},{theme,language});
  for(const view of ['home','positions','assets','watchlist','trends','news','history','settings','tx','market','import','nav','stock-detail','fund-detail']){
   await navigate(view);await page.locator('#view-'+view).waitFor({state:'visible'});
   if(view==='settings')await page.waitForFunction(()=>!!document.querySelector('.settings-overview-grid'));
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   const facts=await page.locator('#view-'+view).evaluate(el=>({width:innerWidth,documentWidth:document.documentElement.scrollWidth,viewWidth:el.clientWidth,viewScroll:el.scrollWidth,text:el.innerText}));
   assert.ok(facts.documentWidth<=facts.width+1,JSON.stringify({theme,language,width,view,facts}));
   assert.ok(facts.viewScroll<=facts.viewWidth+1,JSON.stringify({theme,language,width,view,facts}));
   assert.doesNotMatch(facts.text,/undefined|fund(?:Home|Detail|Decision|Settings|Market|Watch|Ledger|Import)[A-Z]/);
   if(language==='en')assert.doesNotMatch(facts.text,/[\u4e00-\u9fff]/,'English view '+view);
   await page.locator('#view-'+view).screenshot({path:resolve(out,`${view}-${theme}-${language}-${width}.png`)});
   results.push({theme,language,width,view,widths:[facts.viewWidth,facts.viewScroll]});
  }
 }
 await navigate('market');await page.locator('[data-stock-open]').first().press('Enter');await page.locator('#view-stock-detail').waitFor({state:'visible'});
 await navigate('assets');assert.equal(await page.locator('.offline-asset-card').count(),2);assert.equal(await page.locator('#assets-table').count(),0);
 await page.locator('#offline-add').click();await page.locator('dialog[open] [name=name]').fill('Unsaved draft');await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>document.activeElement.id),'offline-add');
 await navigate('news');await page.evaluate(()=>{failAllPageNews=true;return Fund.actions.loadNewsPage();});await page.locator('[data-info-retry]').waitFor();await page.evaluate(()=>failAllPageNews=false);await page.locator('[data-info-retry]').click();await page.locator('[data-info-ai]').waitFor();
 await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));assert.doesNotMatch(await page.locator('#news-page-body').innerText(),/Fixture announcement/);
 await page.evaluate(()=>{const report={validation:{status:'structure_verified',result:{facts:[{text:'<img src=x>',evidenceRefs:['E1']}],impacts:[],counterEvidence:[],gaps:[],options:[]}}};document.getElementById('home-ai-body').innerHTML=Fund.home.renderDecisionResult(report);});
 assert.equal(await page.locator('#home-ai-body img').count(),0);
 assert.deepEqual(errors,[]);
 console.log(`All investment views: ${results.length} theme/language/width cases, cards, errors, privacy and focus passed`);
}finally{writeFileSync(resolve(out,'report.json'),JSON.stringify({results},null,2));await browser.close();}
