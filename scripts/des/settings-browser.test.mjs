// Chrome UI checks with synthetic transport; no Native, credentials or external send.
import assert from 'node:assert/strict';
import {chromium} from 'playwright-core';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
import {shellHtml,shellScripts} from './shell-harness.mjs';
const root=resolve(import.meta.dirname,'../..'),dist=resolve(root,'modules/fund/ui/dist');
const out=resolve(root,'.tmp/des-settings');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const report={scope:'synthetic-settings-ui',checks:[],errors:[]};
try {
 const page=await browser.newPage({viewport:{width:1280,height:900}});
 page.on('pageerror',e=>report.errors.push(e.message));
 await page.setContent(shellHtml());
 await page.evaluate(()=>{
  window.settings={colorMode:'cn',linkPlatform:'eastmoney',sourcePriority:'em_stock_ann',benchmarkSymbol:'sh000300',quoteRefreshSeconds:10,version:1};
  window.policy={silenceStart:'22:00',silenceEnd:'08:00',retryMax:1,backoffBase:3,dedupMinutes:60,version:1};
  window.requests=[];window.failWrite=false;
  window.fetch=async(url,options={})=>{
   const path=String(url).split('?')[0];requests.push({url:String(url),method:options.method||'GET'});
   if(options.method==='POST'&&path==='/api/settings'){
    if(failWrite)return new Response(JSON.stringify({error:'FIN_CONFLICT'}),{status:409});
    settings={...JSON.parse(options.body).settings,version:settings.version+1};
   }
   const body=path==='/api/settings'?settings:path==='/api/notify/settings'?policy:path==='/api/notify/channels'?{channels:[]}:path==='/api/information/sources'?{sources:[]}:path==='/api/opportunities/stats'?{feedback:{total:0,usefulRatePpm:null}}:path==='/api/settings/health'?{databaseOk:true,schemaVersion:1,dataRevision:'1'}:{};
   return new Response(JSON.stringify(body),{status:200});
  };
 });
 for(const file of shellScripts()) {
  if(file==='settings-view.js')await page.evaluate(()=>Fund.home.requestCapability=async()=>({permission:'unknown'}));
  await page.addScriptTag({path:resolve(dist,file)});
 }
 await page.evaluate(()=>{Fund.applyLanguage('zh-CN');Fund.actions.openFinanceSettings();});
 await page.locator('[data-category=accounts]').click();
 const form=page.locator('[data-financial-display]');
 await form.locator('[name=quoteRefreshSeconds]').selectOption('30');
 await page.locator('[data-category=notify]').click();
 await page.locator('[data-category=accounts]').click();
 assert.equal(await form.locator('[name=quoteRefreshSeconds]').inputValue(),'30');
 await page.evaluate(()=>Fund.applyLanguage('en'));
 assert.equal(await form.locator('[name=quoteRefreshSeconds]').inputValue(),'30');
 assert.equal(await form.getAttribute('data-dirty'),'true');
 assert.match(await form.locator('[data-save-status]').innerText(),/Unsaved/i);
 report.checks.push('category and language changes preserve dirty drafts');
 await page.evaluate(()=>failWrite=true);
 await form.locator('[type=submit]').click();
 await form.locator('[data-save-status]').getByText(/changed|conflict/i).waitFor();
 assert.equal(await form.locator('[name=quoteRefreshSeconds]').inputValue(),'30');
 assert.equal(await form.getAttribute('data-dirty'),'true');
 await page.evaluate(()=>failWrite=false);
 await form.locator('[type=submit]').click();
 await form.locator('[data-save-status]').getByText(/Saved/i).waitFor();
 assert.equal(await form.getAttribute('data-dirty'),'false');
 report.checks.push('409 keeps draft; explicit retry saves only this form');
 await form.locator('[name=colorMode]').selectOption('intl');
 await form.locator('[type=submit]').click();
 await page.waitForFunction(()=>document.documentElement.dataset.colorMode==='intl');
 report.checks.push('international financial color mode projects its value');
 for(const theme of ['archive','volt'])for(const width of [320,640,1280,1440]){
  await page.setViewportSize({width,height:900});
  await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;},theme);
  for(const category of ['allocation','accounts','strategy','ai','sources','notify','system','transfer']){
   if(width<=640)await page.locator('#financial-settings-category').selectOption(category);
   else await page.locator(`[data-category=${category}]`).click();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true,`${theme}/${width}/${category}`);
  }
 }
 assert.equal(report.errors.length,0);
 report.checks.push('8 categories × 2 themes × 4 widths have no document overflow');
 await page.screenshot({path:resolve(out,'settings-volt-1440.png'),fullPage:true});
}finally{await browser.close();writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2)+'\n');}
console.log(`settings UI: ${report.checks.length} checks, ${report.errors.length} errors`);
