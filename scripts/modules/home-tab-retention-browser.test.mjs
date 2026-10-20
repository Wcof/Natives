// Actual Chrome visibility transitions; synthetic API, no user ledger.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFileSync,mkdirSync,mkdtempSync,existsSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {chromium} from 'playwright-core';
import {installHomeFixture} from './home-browser-fixture.mjs';
const root=resolve(import.meta.dirname,'../..'),ui=join(root,'modules/fund/ui/src'),out=join(root,'.tmp/home-tab-retention');
mkdirSync(out,{recursive:true});
const profile=mkdtempSync(join(out,'profile-'));
const chrome=spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',['--headless=new','--remote-debugging-port=0',`--user-data-dir=${profile}`,'--no-first-run'],{stdio:'ignore'});
let browser;
const report={scope:'real-Chrome-visibility-synthetic-API',checks:[],errors:[]};
try{
 const portFile=join(profile,'DevToolsActivePort'),deadline=Date.now()+15000;
 while(!existsSync(portFile)){assert.ok(Date.now()<deadline,'Chrome startup');await new Promise(r=>setTimeout(r,100));}
 browser=await chromium.connectOverCDP(`http://127.0.0.1:${readFileSync(portFile,'utf8').split('\n')[0]}`,{noDefaults:true});
 const context=browser.contexts()[0],page=await context.newPage();
 page.on('pageerror',e=>report.errors.push(e.message));
 async function seed(target){
  await target.setContent(`<style>${readFileSync(join(root,'modules/fund/ui/dist/styles/theme.css'),'utf8')}${readFileSync(join(ui,'styles/home.css'),'utf8')}${readFileSync(join(ui,'styles/investment.css'),'utf8')}</style>${readFileSync(join(ui,'partials/view-home.html'),'utf8')}`);
  await target.evaluate(installHomeFixture);
  for(const file of ['core.js','home-i18n.js','home-overview.js','home-opportunities.js','home.js','home-range.js','home-manage.js','home-evidence.js'])await target.addScriptTag({path:join(ui,file)});
 }
 await seed(page);
 await page.bringToFront();await page.evaluate(()=>{Fund.state.token='synthetic';return Fund.actions.loadHome();});
 await page.waitForFunction(()=>requests.some(r=>r.path==='/api/check/run')&&document.getElementById('home-today-body').getAttribute('aria-busy')==='false');
 await page.locator('#home-finance-details').evaluate(el=>el.open=true);
 await page.locator('.home-history > summary').click();
 await page.locator('[data-range=custom]').click();
 await page.locator('[data-date="2026-09-01"]:not(.range-outside)').click();
 await page.waitForTimeout(100);
 await page.evaluate(()=>{window.retainedSnapshot=Fund.home.snapshot();window.retainedGoal=document.getElementById('home-goal-body').firstElementChild;});
 const before=await page.evaluate(()=>requests.length);
 const other=await context.newPage();await other.goto('data:text/html,<title>Visibility fixture</title>');await other.bringToFront();
 await page.waitForFunction(()=>document.hidden);
 await page.waitForTimeout(2500);
 await page.bringToFront();await page.waitForFunction(()=>!document.hidden);await page.waitForTimeout(300);
 const facts=await page.evaluate(n=>({reads:requests.slice(n).filter(r=>/\/api\/home\/snapshot|\/api\/check\/run|\/api\/market\/indices/.test(r.path)).map(r=>r.path),sameSnapshot:retainedSnapshot===Fund.home.snapshot(),sameGoal:retainedGoal===document.getElementById('home-goal-body').firstElementChild,draft:document.querySelector('[data-range-preview]').textContent,expanded:document.getElementById('home-finance-details').open}),before);
 report.checks.push(facts);
 assert.deepEqual(facts.reads,[],'switching browser tabs must not regenerate the confirmed home snapshot or rerun checks');
 assert.equal(facts.sameSnapshot,true);assert.equal(facts.sameGoal,true);assert.match(facts.draft,/2026/);assert.equal(facts.expanded,true);
 await page.locator('#home-refresh-btn').click();
 await page.waitForFunction(n=>requests.slice(n).some(r=>r.path.startsWith('/api/home/snapshot/generate')),before);
 report.checks.push({manualRefresh:true});
 const cold=await context.newPage();await seed(cold);await cold.bringToFront();
 await cold.evaluate(()=>{Fund.state.token='synthetic';window.holdHomeReads=true;Fund.actions.loadHome();});
 await cold.waitForFunction(()=>heldHomeRequests.length===1);
 await other.bringToFront();await cold.waitForFunction(()=>document.hidden);
 assert.equal(await cold.evaluate(()=>heldHomeRequests[0].signal.aborted),true);
 await cold.evaluate(()=>{holdHomeReads=false;heldHomeRequests[0].resolve();});
 await cold.bringToFront();await cold.waitForFunction(()=>requests.some(r=>r.path==='/api/check/run')&&document.getElementById('home-today-body').getAttribute('aria-busy')==='false');
 assert.ok(await cold.evaluate(()=>Fund.home.snapshot()));
 report.checks.push({interruptedInitialLoadResumes:true});
 assert.deepEqual(report.errors,[]);console.log('home short-tab retention: real hidden/visible, same snapshot/DOM/draft, manual refresh passed');
}catch(error){report.failure=error.message;throw error;}
finally{writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');await browser?.close();chrome.kill();}
