// Real Chrome, synthetic watchlist transport. No user database.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
import {shellHtml} from './shell-harness.mjs';
const root=resolve(import.meta.dirname,'../..'),dist=resolve(root,'modules/fund/ui/dist'),out=resolve(root,'.tmp/des-watchlist');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const report={checks:[],errors:[]};
try{
 const page=await browser.newPage({viewport:{width:1280,height:900}});page.on('pageerror',e=>report.errors.push(e.message));
 await page.setContent(shellHtml());
 for(const file of ['core.js','home-i18n.js','research-series.js'])await page.addScriptTag({path:resolve(dist,file)});
 await page.evaluate(()=>{
  Fund.applyLanguage('en');document.querySelectorAll('.main-column>section').forEach(el=>el.hidden=el.id!=='view-watchlist');
  window.calls=[];window.failRead=false;window.hold=false;window.pending=[];
  Fund.actions.selectSymbol=sym=>Fund.state.currentSymbol=sym;
  Fund.actions.updateFavButtonState=()=>{};Fund.actions.updateKPI=()=>{};
  Fund.api=async(method,url,body,options={})=>{
   calls.push({method,url,body});
   if(url==='/api/watchlists')return {groups:[{id:'default',name:'Custom actual group'},{id:'other',name:'Other actual group'}],items:[{watchlistId:'default',symbol:'sh600519',name:'A'},{watchlistId:'default',symbol:'sh600036',name:'B'},{watchlistId:'other',symbol:'sz000001',name:'C'}]};
   if(url.startsWith('/api/market/quotes')){
    if(failRead)throw new Error('Synthetic failure');
    if(hold)return new Promise(resolve=>pending.push({resolve,signal:options.signal}));
    return {quotes:[{code:'sh600519',price:10},{code:'sh600036',price:null},{code:'sz000001',price:20}]};
   }
   return {points:[]};
  };
 });
 await page.addScriptTag({path:resolve(dist,'watchlist-table.js')});
 await page.evaluate(()=>Fund.actions.refreshWatchlist());await page.locator('#watchlist-table tbody tr[data-symbol]').first().waitFor();
 assert.equal(await page.locator('#watchlist-groups button').count(),2);assert.match(await page.locator('#watchlist-groups').innerText(),/Custom actual group/);
 const before=await page.evaluate(()=>calls.length);await page.locator('#watchlist-table th[data-key=price] button').click();
 assert.equal(await page.evaluate(()=>calls.length),before);assert.equal(await page.locator('#watchlist-table tbody tr').last().getAttribute('data-symbol'),'sh600036');
 assert.equal(await page.locator('#watchlist-table th[data-key=price]').getAttribute('aria-sort'),'descending');
 report.checks.push('actual group names; keyboard button sorting without fetch; missing prices last');
 await page.evaluate(()=>{failRead=true;Fund.actions.renderWatchlistTable();});await page.locator('#watchlist-status button').waitFor();
 assert.equal(await page.locator('#watchlist-table tbody tr[data-symbol]').count(),2);report.checks.push('read failure keeps confirmed rows and retry');
 await page.evaluate(()=>{failRead=false;hold=true;Fund.actions.renderWatchlistTable();});await page.waitForFunction(()=>pending.length===1);
 await page.locator('#watchlist-groups [data-id=other]').click();await page.waitForFunction(()=>pending.length===2);
 assert.equal(await page.evaluate(()=>pending[0].signal.aborted),true);
 await page.evaluate(()=>pending[1].resolve({quotes:[{code:'sz000001',price:30}]}));await page.locator('#watchlist-table tr[data-symbol=sz000001]').waitFor();
 await page.evaluate(()=>pending[0].resolve({quotes:[{code:'sh600519',price:99}]}));
 assert.equal(await page.locator('#watchlist-table tbody tr[data-symbol]').count(),1);assert.equal(await page.locator('#watchlist-table tbody tr').first().getAttribute('data-symbol'),'sz000001');
 report.checks.push('group changes cancel old quote projection');
 await page.evaluate(()=>{hold=false;});
 await page.addScriptTag({path:resolve(dist,'trends-chart.js')});
 await page.evaluate(()=>{document.querySelectorAll('.main-column>section').forEach(el=>el.hidden=el.id!=='view-trends');Fund.trends.coordinator={selectStock(){}};Object.assign(Fund.trends.state,{currentSymbol:'sz000001',currentStockName:'C'});});
 await page.addScriptTag({path:resolve(dist,'trends-watch.js')});
 const mutationCount=await page.evaluate(()=>calls.filter(call=>call.method==='DELETE').length);
 await page.locator('#star-btn').click();await page.waitForFunction(()=>!document.getElementById('star-btn').disabled);
 assert.equal(await page.evaluate(()=>calls.filter(call=>call.method==='DELETE').length),mutationCount+1,'one click has one real owner mutation');
 const remove=await page.evaluate(()=>calls.filter(call=>call.method==='DELETE').at(-1));assert.match(remove.url,/watchlistId=other&symbol=sz000001/);
 assert.equal(await page.evaluate(()=>typeof Fund.trends.watch.saveWatchlist),'undefined','research exposes no second watchlist writer');
 report.checks.push('research star uses actual selected group, one owner mutation per click and no local duplicate');
 assert.equal(report.errors.length,0);
}finally{await browser.close();writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');}
console.log(`watchlist: ${report.checks.length} checks, ${report.errors.length} errors`);
