// UI contract only: synthetic API/broker, no real provider or holdings.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {chromium} from 'playwright-core';
const root=resolve(import.meta.dirname,'../..');
const chrome=process.env.NATIVES_TEST_CHROME;
assert.ok(chrome,'NATIVES_TEST_CHROME is required');
const browser=await chromium.launch({executablePath:chrome,headless:true});
try{
 const page=await browser.newPage();
 await page.setContent('<div id="home-ai-body" tabindex="0"></div><button data-act="news-ai" data-id="1"></button>');
 await page.evaluate(()=>{
  window.__stats={starts:0,contentFetches:0,cancels:0,hold:false,mode:"amount",defaultModel:{providerId:'p',modelId:'m',name:'M',configToken:'t'}};
  const home={
   escape:v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])),
   panel:()=>null,failure:()=>{},status:()=>{},requestId:(()=>{let n=0;return()=>String(++n)})(),
   displayMode:()=> __stats.mode,snapshot:()=>({dataRevision:1}),readSnapshot:async()=>({dataRevision:1}),
   requestInformation:async()=>{window.__stats.contentFetches++;return {ok:true};},
   informationSourceLabel:()=> 'source',informationTimeLabel:()=> 'time',informationContentLabel:()=> 'full',
   closePanel:()=>{},save:()=>'',select:()=>'',check:()=>'',input:()=>'',text:()=>'',submit:()=>{},
  };
  window.Fund={t:key=>key,home,api:async(method,path,payload)=>{
   if(path.startsWith('/api/information/item')){const id=Number(new URL('http://x'+path).searchParams.get('id'));return {id,title:'Synthetic news',sourceId:'em_finance',publishedAt:'2026-10-20T00:00:00Z',contentText:id===3?'':'Synthetic body',contentFetchedAt:id===3?null:'2026-10-20T00:00:00Z',completeness:id===3?'excerpt':'full',contentVersion:1,contentSupported:true};}
   if(path==='/api/home/evidence'){const summary=payload?.informationIds?.[0]===3;return {snapshotId:'synthetic',dataRevision:1,scope:'portfolio',task:'news_impact',instructions:'test',analysisPolicy:{source:'built-in'},evidence:[{id:'I'+(payload?.informationIds?.[0]||1),kind:'information',title:'Synthetic news',basis:summary?'summary':'content',contentSentChars:summary?0:14,contentTotalChars:14,contentTruncated:false,fullTextRead:!summary,summary:'Synthetic summary',coverageNotice:''}]};}
   if(path==='/api/settings')return {aiMaxTokens:4096};
   if(path==='/api/analysis/validate')return {status:'structure_verified',result:{facts:[],impacts:[],options:[],counterEvidence:[],gaps:[]}};
   throw Error('unexpected synthetic API '+path);
  }};
  window.addEventListener('message',event=>{
   const msg=event.data;if(msg?.type!=='natives:ai')return;
   const reply=result=>window.postMessage({type:'natives:reply',requestId:msg.requestId,generation:msg.generation,challenge:msg.challenge,result},'*');
   if(msg.op==='options')return reply({models:[{providerId:'p',modelId:'m',name:'M'}],defaultModel:window.__stats.defaultModel});
   if(msg.op==='start'){window.__stats.starts++;return reply({providerId:'p',modelId:'m'});}
   if(msg.op==='status')return reply({done:!__stats.hold,phase:'writing',text:'Synthetic result'});
   if(msg.op==='cancel'){__stats.cancels++;return reply({cancelled:true});}
   reply({});
  });
 });
 await page.addScriptTag({path:resolve(root,'modules/fund/ui/src/home-evidence.js')});
 await page.evaluate(()=>window.postMessage({type:'init',generation:'g',challenge:'c'},'*'));
 await page.evaluate(()=>Fund.home.analyzeNewsForAI({id:1,title:'Synthetic news',sourceId:'em_finance',publishedAt:'2026-10-20T00:00:00Z'}));
 await page.locator('[data-act=home-ai-record]').waitFor({timeout:10000});
 assert.equal(await page.locator('[data-act=home-ai-confirm]').count(),0,'no preview confirmation');
 assert.equal(await page.evaluate(()=>__stats.starts),1,'one click must send one inference');
 await page.evaluate(()=>Fund.home.analyzeNewsForAI({id:1,title:'Synthetic news',sourceId:'em_finance'}));
 await page.waitForTimeout(100);
 assert.equal(await page.evaluate(()=>__stats.starts),1,'repeated click must not resend');
 await page.evaluate(()=>document.dispatchEvent(new Event('fund:home-snapshot-updated')));
 await page.waitForTimeout(100);
 assert.equal(await page.evaluate(()=>__stats.starts),1,'snapshot refresh must not auto-infer');
 await page.evaluate(()=>Fund.home.analyzeNewsForAI({id:3,title:'Summary news',sourceId:'em_finance',publishedAt:'2026-10-20T00:00:00Z'}));
 await page.waitForFunction(()=>document.querySelector('[data-act=home-ai-record]'));
 assert.equal(await page.evaluate(()=>__stats.contentFetches),1,'missing body must be fetched automatically once');
 assert.equal(await page.evaluate(()=>__stats.starts),2,'summary fallback still starts one inference');
 await page.evaluate(()=>{__stats.defaultModel=null;Fund.home.analyzeNewsForAI({id:2,title:'Second news',sourceId:'em_finance',publishedAt:'2026-10-20T00:00:00Z'});});
 await page.locator('[data-act=home-ai-model-settings]').waitFor({timeout:10000});
 assert.equal(await page.evaluate(()=>__stats.starts),2,'no first-model fallback');
 await page.evaluate(()=>{__stats.defaultModel={providerId:'p',modelId:'m',name:'M',configToken:'t'};__stats.hold=true;Fund.home.analyzeNewsForAI({id:4,title:'Pending news',sourceId:'em_finance'});});
 await page.waitForFunction(()=>__stats.starts===3);
 await page.evaluate(()=>{__stats.mode='percent';document.dispatchEvent(new Event('fund:display-mode-changed'));});
 assert.equal(await page.evaluate(()=>Fund.home.inlineAIActive()),true,'percentage display must preserve pending inference');
 assert.equal(await page.evaluate(()=>__stats.cancels),0);
 await page.evaluate(()=>{__stats.mode='hidden';document.dispatchEvent(new Event('fund:display-mode-changed'));});
 await page.waitForFunction(()=>__stats.cancels===1);
 assert.equal(await page.evaluate(()=>Fund.home.inlineAIActive()),false);
 await page.evaluate(()=>{__stats.mode='amount';document.dispatchEvent(new Event('fund:display-mode-changed'));});
 assert.match(await page.locator('#home-ai-body').innerText(),/fundHomeAiPrivacyCancelled/);
 assert.equal(await page.evaluate(()=>__stats.starts),3,'restoring visibility must not resend');
 console.log('one-click UI contract passed');
}finally{await browser.close();}
