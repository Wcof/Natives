// Actual Chrome pointer/keyboard gestures; persistence replies are isolated synthetic data.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
import {shellHtml} from './shell-harness.mjs';
const root=resolve(import.meta.dirname,'../..'),dist=resolve(root,'modules/fund/ui/dist'),out=resolve(root,'.tmp/des-drawing-gestures');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const report={scope:'real-Chrome-gestures-synthetic-API',steps:[],errors:[]};
try{
 const page=await browser.newPage({viewport:{width:1280,height:900}});page.on('pageerror',e=>report.errors.push(e.message));
 await page.setContent(shellHtml());
 for(const file of ['core.js','home-i18n.js','research-series.js','drawing-session.js','echarts.min.js','trends-chart.js','trends-kline.js','stock-drawings-shapes.js','trends-drawings.js'])await page.addScriptTag({path:resolve(dist,file)});
 await page.evaluate(()=>{
  window.remote=[];window.saves=[];
  window.fetch=async(url,options={})=>{
   const method=options.method||'GET';
   if(method==='GET')return new Response(JSON.stringify({drawings:remote}));
   if(method==='DELETE'){const id=new URL(url,location.href).searchParams.get('id');remote=remote.filter(d=>d.id!==id);return new Response('{"success":true}');}
   const body=JSON.parse(options.body);saves.push(body);const drawing={...body,digest:'a'.repeat(64)};remote=[...remote.filter(d=>d.id!==body.id),drawing];return new Response(JSON.stringify({success:true,drawing}));
  };
  document.querySelectorAll('.main-column>section').forEach(node=>node.hidden=node.id!=='view-trends');document.querySelector('.workspace-body').classList.add('trends-active');
  document.getElementById('trends-empty').hidden=true;
  Object.assign(Fund.trends.state,{currentSymbol:'sh600519',instrumentKind:'stock',currentView:'dayk'});Fund.trends.chart.initCharts();
  Fund.trends.chart.renderKline(Array.from({length:60},(_,i)=>({date:new Date(Date.UTC(2026,7,1+i)).toISOString().slice(0,10),open:10,close:12,low:8,high:14,volume:1000})),{},false);
  Fund.trends.drawings.init();
 });
 await page.waitForTimeout(100);
 const point=async(index,price)=>{const xy=await page.evaluate(({index,price})=>Fund.trends.charts.klineChart.convertToPixel({xAxisIndex:0,yAxisIndex:0},[Fund.trends.state.klineData[index].date,price]),{index,price});const box=await page.locator('#kline-chart').boundingBox();return{x:box.x+xy[0],y:box.y+xy[1]};};
 const drag=async(a,b)=>{await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:8});await page.mouse.up();};
 const a=await point(20,10),b=await point(35,12),c=await point(28,13);
 for(const tool of ['trendline','horizontal','rectangle','channel','polyline','circle','fibonacci','measure','text']){
  await page.evaluate(async()=>{remote=[];await Fund.trends.drawings.reload();});
  const before=await page.evaluate(()=>saves.length);await page.locator('.td-btn[data-tool="'+tool+'"]').click();
  if(tool==='horizontal')await page.mouse.click(a.x,a.y);
  else if(tool==='polyline'){await page.mouse.click(a.x,a.y);await page.mouse.move(b.x,b.y);await page.mouse.click(b.x,b.y);await page.mouse.move(c.x,c.y);await page.mouse.click(c.x,c.y);await page.keyboard.press('Enter');}
  else if(tool==='text'){
   await page.mouse.click(a.x,a.y);await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>saves.length),before,'cancelled text must not persist');
   await page.locator('.td-btn[data-tool=text]').click();await page.mouse.click(a.x,a.y);await page.locator('dialog[open] input').fill('Isolated annotation');await page.locator('dialog[open] [type=submit]').click();
  }else{await drag(a,b);if(tool==='channel'){assert.equal(await page.evaluate(()=>saves.length),before,'channel baseline is only a draft');await page.mouse.click(c.x,c.y);}}
  await page.waitForFunction(n=>saves.length===n+1,before);
  await page.waitForFunction(()=>document.querySelector('#trends-chart-toolbar .drawing-status')?.textContent===Fund.t('fundChartDrawingSaved'));
  const saved=await page.evaluate(()=>saves.at(-1));assert.equal(saved.toolType,tool);assert.equal(saved.options.coordinateVersion,2);assert.equal(saved.options.viewContext.fq,'qfq');assert.ok(saved.points.every(p=>/^2026-\d{2}-\d{2}$/.test(p.date)&&Number.isFinite(p.price)));assert.equal(saved.options.color,undefined);
  const index=await page.evaluate(date=>Fund.trends.state.klineData.findIndex(row=>row.date===date),saved.points[0].date);
  const anchor=await point(index,saved.points[0].price),target=await point(index+2,saved.points[0].price+0.3);
  await drag(anchor,target);await page.waitForFunction(n=>saves.length===n+2,before);
  await page.waitForFunction(()=>document.querySelector('#trends-chart-toolbar .drawing-status')?.textContent===Fund.t('fundChartDrawingSaved'));
  const edited=await page.evaluate(()=>saves.at(-1));assert.equal(edited.id,saved.id);assert.equal(edited.expectedDigest,'a'.repeat(64));assert.notDeepEqual(edited.points,saved.points);
  report.steps.push({tool,points:saved.points.length,anchorEdit:true,pass:true});
  if(tool==='trendline'){
   const original=await page.evaluate(()=>Fund.trends.charts.klineChart.getWidth());
   await page.evaluate(()=>{document.querySelector('#view-trends .chart-container').style.marginRight='32px';});
   await page.waitForFunction(original=>{const c=Fund.trends.charts.klineChart;return c.getWidth()<original-20&&Math.abs(c.getWidth()-c.getDom().clientWidth)<1;},original);
   await page.waitForFunction(point=>{const c=Fund.trends.charts.klineChart,px=c.convertToPixel({xAxisIndex:0,yAxisIndex:0},[point.date,point.price]),anchor=c.getOption().graphic[0].elements.find(e=>e.id.endsWith('_anchor_0'));return anchor&&Math.abs(anchor.shape.cx-px[0])<1&&Math.abs(anchor.shape.cy-px[1])<1;},edited.points[0]);
   await page.evaluate(()=>{document.querySelector('#view-trends .chart-container').style.marginRight='';});
   await page.waitForFunction(original=>Math.abs(Fund.trends.charts.klineChart.getWidth()-original)<1,original);
   report.steps.push({tool:'container-resize-without-window-event',anchorReprojected:true,pass:true});
  }

 }
 const before=await page.evaluate(()=>saves.length);await page.locator('.td-btn[data-tool=rectangle]').click();await page.mouse.click(a.x,a.y);assert.equal(await page.evaluate(()=>saves.length),before,'zero-area rectangle must not save');
 await page.locator('.td-btn[data-tool=channel]').click();await drag(a,b);await page.keyboard.press('Escape');await page.mouse.click(c.x,c.y);assert.equal(await page.evaluate(()=>saves.length),before,'Escape cancels channel draft');
 report.steps.push({tool:'cancel-and-zero-area',pass:true});assert.deepEqual(report.errors,[]);
}finally{await browser.close();writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');}
console.log('Drawing gestures: '+report.steps.length+' checks; '+report.errors.length+' page errors');
