// Real Chrome rendering with isolated public-metadata responses, never user holdings.
import assert from 'node:assert/strict';
import {resolve} from 'node:path';
import {readFileSync} from 'node:fs';
import {chromium} from 'playwright-core';
const root=resolve(import.meta.dirname,'../..'),ui=resolve(root,'modules/fund/ui/src');
const messages=Object.fromEntries(['zh_CN','en'].map(lang=>[lang,JSON.parse(readFileSync(resolve(root,'extension/_locales',lang,'messages.json'),'utf8'))]));
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
try{
 const page=await browser.newPage({viewport:{width:320,height:700}}),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.setContent('<section id="view-positions"><div data-fund-classification="903081"></div><div data-fund-classification="903082"></div><div data-fund-classification="903083"></div><div data-fund-classification="903084"></div></section>');
 await page.evaluate(messages=>{
  window.mode='amount';window.failure=true;window.calls=[];window.pending=null;
  window.Fund={state:{language:'zh-CN'},home:{displayMode:()=>mode},t:(key,args={})=>(messages[Fund.state.language==='en'?'en':'zh_CN'][key]?.message||key).replace(/\{(\w+)\}/g,(_,k)=>args[k]??`{${k}}`),api:async(_method,path,_body,options)=>{
   const code=path.split('=')[1];calls.push(code);
   if(code==='903083')return await new Promise(resolve=>{pending={resolve,signal:options.signal};});
   if(code==='903082'&&failure)throw new Error('isolated source failure');
   return {code,status:code==='903081'?'unclassified':'ready',themes:['903081','903084'].includes(code)?[]:[{name:'<img src=x onerror="window.xss=true">'}],indexName:code==='903084'?'Public low-volatility dividend index':null,sourceUrl:'https://fund.eastmoney.com/'+code+'.html',fetchedAt:Date.now()/1000};
  }};
 },messages);
 await page.addScriptTag({path:resolve(ui,'fund-classification.js')});
 await page.evaluate(()=>Fund.fundClassification.mount(document.getElementById('view-positions')));
 await page.waitForFunction(()=>pending&&document.querySelector('[data-fund-classification="903082"] button'));
 assert.match(await page.locator('[data-fund-classification="903081"]').innerText(),/未提供/);
 assert.match(await page.locator('[data-fund-classification="903082"]').innerText(),/失败/);
 assert.match(await page.locator('[data-fund-classification="903084"]').innerText(),/跟踪指数/);
 await page.evaluate(()=>{failure=false;});await page.locator('[data-fund-classification="903082"] button').press('Enter');
 await page.locator('[data-fund-classification="903082"] .fund-theme-tag').waitFor();
 assert.equal(await page.locator('[data-fund-classification="903082"] a').evaluate(node=>node===document.activeElement),true,'keyboard retry restores focus to the resolved source link');
 assert.equal(await page.locator('[data-fund-classification="903082"] img').count(),0);
 assert.equal(await page.evaluate(()=>window.xss||false),false);
 assert.equal(await page.locator('[data-fund-classification="903082"] a').getAttribute('rel'),'noopener noreferrer');
 await page.evaluate(()=>{Fund.state.language='en';Fund.fundClassification.mount(document.getElementById('view-positions'));});
 assert.match(await page.locator('[data-fund-classification="903081"]').innerText(),/No fund theme/);
 await page.evaluate(()=>{window.cancelledSignal=pending.signal;mode='hidden';Fund.fundClassification.mount(document.getElementById('view-positions'));});
 assert.equal(await page.evaluate(()=>cancelledSignal.aborted),true);
 assert.equal(await page.locator('[data-fund-classification="903082"]').isVisible(),false);
 assert.equal(await page.locator('a').count(),0,'privacy removes source identity');
 assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
 assert.deepEqual(errors,[]);
 console.log('fund themes: empty / explicit source failure / keyboard retry / unsafe labels / source link / language / cancellation / privacy passed');
}finally{await browser.close();}
