// Real Chrome layout regression; public research responses are explicit synthetic fixtures.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {shellHtml,shellScripts} from './shell-harness.mjs';
import {installHomeFixture} from '../modules/home-browser-fixture.mjs';

const root=resolve(import.meta.dirname,'../..'),dist=resolve(root,'modules/fund/ui/dist');
const out=resolve(root,'.tmp/trends-layout');mkdirSync(out,{recursive:true});
const name='Synthetic research with a long instrument name · ETF / 股票测试';
const rows=Array.from({length:80},(_,i)=>({date:new Date(Date.UTC(2026,6,i+1)).toISOString().slice(0,10),open:10,close:11+i/100,low:9,high:12,volume:1000}));
const analyze={symbol:'sh600519',name,quote:{symbol:'sh600519',name,price:1234.56,change:12.34,pct:1.01,high:1250,low:1200,volume:10000000,time:'2026-10-10 15:00:00'},klines:rows,signal:{action:'观望',score:50,modules:[],breakouts:[],key_levels:{},risk_notes:[]},flows:[]};
const search={results:[{symbol:'sh600519',code:'600519',name,market:'SSE',instrumentKind:'stock'}]};

export async function verifyTrendsLayout(page,surface,runtime,report,{native=false}={}){
  await page.bringToFront();
  if(native){
    await page.route('**/api/trends/search?*',route=>route.fulfill({json:search}));
    await page.route('**/api/trends/analyze?*',route=>route.fulfill({json:analyze}));
    await page.route('**/api/trends/chanlun_daily?*',route=>route.fulfill({json:{error:'synthetic_unavailable'}}));
  }
  await surface.locator('#tab-trends').click();
  await surface.locator('#search-input').fill('600519');
  await surface.locator('#search-results button').first().waitFor();
  await surface.locator('#search-input').press('ArrowDown');
  await surface.locator('#search-input').press('Enter');
  await runtime.waitForFunction(()=>Fund.trends.state.klineData.length===80);
  await runtime.evaluate(()=>{Fund.trends.charts.klineChart.dispatchAction({type:'dataZoom',start:20,end:80});window.__layoutChart=Fund.trends.charts.klineChart;});
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const [width,height] of [[1440,900],[1280,600],[640,700],[320,700]]){
    const cdp=await page.context().newCDPSession(page);
    await cdp.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
    await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.applyLanguage(language);},{theme,language});
    await runtime.evaluate(()=>{document.querySelector('#view-trends .main').scrollTop=0;document.querySelector('#view-trends .chart-container').scrollTop=0;});
    await runtime.waitForFunction(()=>{
      const chart=Fund.trends.charts.klineChart,el=chart.getDom();return Math.abs(chart.getWidth()-el.clientWidth)<1&&Math.abs(chart.getHeight()-el.clientHeight)<1;
    });
    const geometry=await runtime.evaluate(()=>{
      const rect=id=>{const r=document.getElementById(id).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
      const controls=[...document.querySelectorAll('#view-trends .topbar button,#search-input,#trends-chart-toolbar button,#trends-fq')].filter(el=>el.getBoundingClientRect().width&&el.closest('#search-results')===null).map(el=>{const r=el.getBoundingClientRect();return {id:el.id||el.textContent,x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};});
      const overlap=[];controls.forEach((a,i)=>controls.slice(i+1).forEach(b=>{if(Math.min(a.right,b.right)-Math.max(a.x,b.x)>1&&Math.min(a.bottom,b.bottom)-Math.max(a.y,b.y)>1)overlap.push([a.id,b.id]);}));
      return {search:rect('search-input'),quote:rect('quote-bar'),main:rect('kline-chart'),volume:rect('volume-chart'),overflow:document.documentElement.scrollWidth>innerWidth+1,overlap,controls,chartSame:window.__layoutChart===Fund.trends.charts.klineChart,zoom:Fund.trends.charts.klineChart.getOption().dataZoom[0]};
    });
    assert.equal(geometry.overflow,false,`${theme}/${language}/${width}: page overflow`);
    assert.deepEqual(geometry.overlap,[],`${width}: controls overlap`);
    assert.ok(geometry.quote.y>=geometry.search.bottom-1,'quote has a separate row');
    assert.ok(geometry.search.width>=Math.min(140,width-80),'search is usable');
    assert.ok(geometry.main.height>=359,'main price chart retains height');
    assert.ok(geometry.volume.height>=89,'volume retains height');
    assert.ok(geometry.controls.every(c=>c.height>=31.9),'32px control targets');
    assert.equal(geometry.chartSame,true,'resize reuses the chart');
    assert.equal(geometry.zoom.start,20);assert.equal(geometry.zoom.end,80);
    await page.screenshot({path:resolve(out,`${native?'native':'synthetic'}-${theme}-${language}-${width}-${height}-price.png`)});
    await runtime.evaluate(()=>window.switchIndicator('macd'));
    await runtime.waitForFunction(()=>Fund.trends.charts.indicatorChart.getHeight()>=159);
    await surface.locator('#indicator-chart').scrollIntoViewIfNeeded();
    await page.screenshot({path:resolve(out,`${native?'native':'synthetic'}-${theme}-${language}-${width}-${height}.png`)});
    await runtime.evaluate(()=>window.switchIndicator('none'));
    await cdp.detach();
    report.steps.push({name:'trends selected layout',native,theme,language,width,height,geometry});
  }
  // Change only the containing element, without dispatching a window resize.
  await runtime.evaluate(()=>{window.__widthBeforeContainerResize=Fund.trends.charts.klineChart.getWidth();document.querySelector('#view-trends .chart-container').style.marginRight='32px';});
  await runtime.waitForFunction(()=>Fund.trends.charts.klineChart.getWidth()<window.__widthBeforeContainerResize-20&&Math.abs(Fund.trends.charts.klineChart.getWidth()-document.getElementById('kline-chart').clientWidth)<1);
  await runtime.evaluate(()=>{document.querySelector('#view-trends .chart-container').style.marginRight='';});
  await surface.locator('#search-input').fill('retry');
  if(native)await page.route('**/api/trends/search?*',route=>route.fulfill({status:503,json:{error:'APP_UPSTREAM'}}));
  else await runtime.evaluate(()=>window.__searchFails=true);
  await surface.locator('#search-input').fill('failure');
  await surface.locator('#search-results p').waitFor();
  await runtime.waitForFunction(()=>document.getElementById('search-results').textContent.includes(Fund.t('fundFocusSearchFailed')));
  assert.equal(await surface.locator('#search-input').inputValue(),'failure');
  await surface.locator('#search-input').press('Escape');
  assert.equal(await surface.locator('#search-results').isVisible(),false);
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
  const report={scope:'synthetic-research-layout',steps:[],errors:[]};
  try{
    const page=await browser.newPage({viewport:{width:1440,height:900}});page.on('pageerror',e=>report.errors.push(e.message));
    let html=shellHtml();
    if(process.argv.includes('--baseline'))for(const file of ['partials/view-trends.html','styles/trends.css'])html=html.replace(readFileSync(resolve(root,'modules/fund/ui/src',file),'utf8'),readFileSync(resolve(out,'modules/fund/ui/src',file),'utf8'));
    await page.setContent(html);await page.evaluate(installHomeFixture);
    await page.evaluate(({analyze,search})=>{const fallback=window.fetch;window.fetch=async(url,options)=>{const path=String(url).split('?')[0];if(path==='/api/trends/search')return new Response(JSON.stringify(window.__searchFails?{error:'APP_UPSTREAM'}:search),{status:window.__searchFails?503:200});if(path==='/api/trends/analyze')return new Response(JSON.stringify(analyze));if(path==='/api/trends/chanlun_daily')return new Response(JSON.stringify({error:'synthetic_unavailable'}));return fallback(url,options);};},{analyze,search});
    await page.addScriptTag({path:resolve(dist,'echarts.min.js')});
    for(const file of shellScripts())await page.addScriptTag({path:resolve(dist,file)});
    await page.evaluate(()=>{Fund.state.token='synthetic';});
    await verifyTrendsLayout(page,page,page,report);
    assert.deepEqual(report.errors,[]);
    console.log('Trends layout: 16 theme/language/width/height cases, search keyboard/error, stable charts and resize passed.');
  }finally{writeFileSync(resolve(out,process.argv.includes('--baseline')?'baseline-report.json':'synthetic-report.json'),JSON.stringify(report,null,2));await browser.close();}
}
