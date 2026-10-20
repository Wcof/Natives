// 192 production-layout combinations in Chrome, synthetic API only.
import {chromium} from 'playwright-core';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
import {shellHtml,shellScripts} from './shell-harness.mjs';
import {installHomeFixture} from '../modules/home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../..'),dist=resolve(root,'modules/fund/ui/dist');
const out=resolve(root,'.tmp/des-page-matrix');mkdirSync(out,{recursive:true});
const report={scope:'synthetic-layout-only',results:[],errors:[]};
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try {
 for(const theme of ['archive','volt'])for(const lang of ['zh-CN','en'])for(const width of [320,640,1280,1440]){
  const page=await browser.newPage({viewport:{width,height:900}});
  page.on('pageerror',e=>report.errors.push({theme,lang,width,message:e.message}));
  await page.setContent(shellHtml());
  await page.evaluate(installHomeFixture);
  await page.evaluate(()=>{
   const fallback=window.fetch;
   const rows=Array.from({length:80},(_,i)=>({date:new Date(Date.UTC(2026,6,i+1)).toISOString().slice(0,10),open:10,close:11,low:9,high:12,volume:1000}));
   const settings={colorMode:'cn',linkPlatform:'eastmoney',sourcePriority:'em_stock_ann',benchmarkSymbol:'sh000300',quoteRefreshSeconds:30,version:1};
   window.fetch=async(url,options)=>{
    const path=String(url).split('?')[0];
    if(path==='/api/home/preferences')return new Response(JSON.stringify({displayMode:'amount',privacyHidden:false,notificationsEnabled:false}),{status:200});
    const extra=path==='/api/positions/detail'?{dataRevision:'synthetic',accounts:[{id:1,name:'示例账户',version:1,archived:false}],holdings:[{accountId:1,fundId:1,fundCode:'600519',fundName:'示例股票',instrumentKind:'stock',venue:'exchange',quoteSymbol:'sh600519',quantity:'100',cost:'1000.00',realized:'0.00',costUnknown:false,marketValue:'1100.00',pendingCount:0,asOf:'2026-09-30',quote:{price:11,asOf:'2026-09-30 15:00'}}],realized:[]}:path==='/api/watchlists'?{groups:[{id:'default',name:'默认分组',sortOrder:0}],items:[],digest:'synthetic'}:path==='/api/offline/assets'||path==='/api/legacy-assets'?{assets:[]}:path==='/api/transactions'?{transactions:[]}:path==='/api/nav'?{funds:[],navs:[],hasMore:false}:path==='/api/market/sectors'?{sectors:[]}:null;
    if(extra?.holdings){const base=extra.holdings[0];extra.holdings=['stock','etf','fund'].map((kind,i)=>({...base,fundId:i+1,instrumentKind:kind,venue:kind==='fund'?'otc':'exchange',fundCode:['600519','510300','005827'][i],fundName:['示例股票','示例场内基金','示例场外基金'][i],quoteSymbol:['sh600519','sh510300',null][i],averageCost:'10.00',holdingDays:30,nav:kind==='fund'?{nav:'11.0000',date:'2026-09-30'}:null,income:Object.fromEntries(['day','yesterday','holding','week','month','year','cumulative'].map((key,j)=>[key,{amount:j===1?'-5.00':'10.00',rateBps:j===1?'-50':'100',date:j===1?'2026-09-29':'2026-09-30'}]))}));}
    if(extra)return new Response(JSON.stringify(extra),{status:200});
    const data=path==='/api/settings'?settings:path==='/api/notify/channels'?{channels:[]}:path==='/api/notify/settings'?{silenceStart:'22:00',silenceEnd:'08:00',retryMax:1,backoffBase:3,dedupMinutes:60,version:1}:path==='/api/settings/health'?{databaseOk:true,schemaVersion:1,dataRevision:'1'}:path==='/api/opportunities/stats'?{feedback:{total:0,usefulRatePpm:null}}:path==='/api/trends/analyze'?{symbol:'sh600519',name:'Synthetic research',quote:{symbol:'sh600519',name:'Synthetic research',price:11,change:1,pct:10},klines:rows,signal:{action:'观望',score:50,modules:[],breakouts:[],key_levels:{},risk_notes:[]},flows:[]}:null;
    return data?new Response(JSON.stringify(data),{status:200}):fallback(url,options);
   };
  });
  await page.addScriptTag({path:resolve(dist,'echarts.min.js')});
  for(const file of shellScripts()){
   if(file==='settings-view.js')await page.evaluate(()=>Fund.home.requestCapability=async()=>({permission:'unknown'}));
   await page.addScriptTag({path:resolve(dist,file)});
  }
  await page.evaluate(({theme,lang})=>{document.documentElement.dataset.theme=theme;Fund.applyLanguage(lang);Fund.state.token='synthetic';},{theme,lang});
  for(const view of ['home','positions','assets','tx','watchlist','market','trends','stock-detail','focus','import','nav','settings']){
   try {
    await page.evaluate(async view=>{
     if(document.body.classList.contains('focus-mode'))Fund.actions.exitFocusMode();
     if(view==='settings'){Fund.actions.openFinanceSettings();return;}
     if(view==='stock-detail'){Fund.actions.openStockDetail('sh600519','Synthetic research');return;}
     if(view==='focus'){document.getElementById('tab-trends').click();await Fund.trends.coordinator.selectStock('sh600519','Synthetic research');Fund.actions.enterFocusMode();return;}
     document.getElementById('tab-'+view)?.click();
    },view);
    await page.waitForTimeout(120);
    const target=view==='focus'?'#focus-workspace':'#view-'+view;
    if(view==='settings')await page.locator('[data-category=allocation]').waitFor({state:'attached'});
    const metrics=await page.evaluate(target=>({visible:!document.querySelector(target).hidden,viewport:innerWidth,document:document.documentElement.scrollWidth,theme:document.documentElement.dataset.theme,renderError:/Cannot read properties|ReferenceError|TypeError/.test(document.querySelector(target).innerText)}),target);
    const pass=metrics.visible&&!metrics.renderError&&metrics.document<=metrics.viewport+1&&metrics.theme===theme;
    report.results.push({theme,lang,width,view,pass,metrics});
    if(lang==='zh-CN'&&[320,1440].includes(width))await page.screenshot({path:resolve(out,`${theme}-${width}-${view}.png`),fullPage:true});
   }catch(e){report.results.push({theme,lang,width,view,pass:false,error:String(e).slice(0,300)});}
  }
  await page.close();
 }
}finally{await browser.close();writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');}
const failures=report.results.filter(r=>!r.pass);console.log(`${report.results.length-failures.length}/${report.results.length} layout combinations; ${report.errors.length} page errors`);
if(failures.length||report.errors.length){console.log(JSON.stringify({failures,errors:report.errors},null,2));process.exitCode=1;}
