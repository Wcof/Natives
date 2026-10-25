// Real isolated Core HTTP authorization and production Web entry in Chrome.
// Only the empty page shell is supplied by the test; authentication is never mocked.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { resolve } from 'node:path';
import { once } from 'node:events';
import { chromium } from 'playwright-core';

const root=resolve(import.meta.dirname,'..'),scratch=resolve(root,'.tmp/no-browser-pairing');
mkdirSync(scratch,{recursive:true});
const out=mkdtempSync(resolve(scratch,'http-')),binary=resolve(out,'native-file-host');
copyFileSync(resolve(root,'target/debug/native-file-host'),binary);
const reserve=createServer();await new Promise(r=>reserve.listen(0,'127.0.0.1',r));
const port=reserve.address().port;await new Promise(r=>reserve.close(r));
const base=`http://127.0.0.1:${port}`;
const env={...process.env,HOME:resolve(out,'home'),NATIVES_STATE_DIR:resolve(out,'state'),NATIVES_WORKSPACE_DB:resolve(out,'workspace.db'),NATIVES_APPS_ROOT:resolve(out,'apps'),NATIVES_DEV_PRODUCT_SOURCE:resolve(out,'product-source'),NATIVES_WEB_ROOT:resolve(root,'extension')};
mkdirSync(env.HOME,{recursive:true});
const retired=spawnSync(binary,['--pair'],{env,encoding:'utf8',timeout:3000});
assert.equal(retired.status,2,'retired CLI must reject without starting Core or reading stdin');
const core=spawn(binary,['--service','--port',String(port)],{env,stdio:['ignore','ignore','pipe']});
let stderr='';core.stderr.on('data',chunk=>{stderr+=chunk;});
let browser;
const checks=[];
try {
  const deadline=Date.now()+10000;
  while(true){
    try{const response=await fetch(`${base}/healthz`,{signal:AbortSignal.timeout(500)});if(response.ok)break;}catch{}
    if(core.exitCode!==null||Date.now()>deadline)throw Error(`isolated Core startup failed: ${stderr}`);
    await new Promise(r=>setTimeout(r,50));
  }
  const secret=readFileSync(resolve(env.NATIVES_STATE_DIR,'relay.secret'),'utf8').trim();
  async function bootstrap(){
    const response=await fetch(`${base}/native/relay`,{method:'POST',headers:{'Content-Type':'application/json','X-Natives-Relay-Secret':secret},body:JSON.stringify({id:'browser-bootstrap',method:'native:bootstrap',params:{}})});
    const data=await response.json();assert.equal(data.ok,true);assert.equal(typeof data.result.bootstrapToken,'string');return data.result.bootstrapToken;
  }
  browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en-US'])for(const width of [1280,320]){
    const context=await browser.newContext({locale:language,viewport:{width,height:800}});
    await context.route('**/space.html*',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><html><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/theme.css"><body><button id="workspace-focus">Workspace</button><script type="module" src="/web-entry.js"></script></body></html>'}));
    const page=await context.newPage(),errors=[],exchanges=[];page.on('pageerror',e=>errors.push(e.message));
    page.on('response',response=>{if(response.url().endsWith('/web/session/exchange'))exchanges.push({status:response.status()});});
    let documentId=0;
    async function load(hash=''){
      await page.goto(`${base}/space.html?document=${++documentId}${hash}`);
      await page.evaluate(async theme=>{document.documentElement.dataset.theme=theme;document.getElementById('workspace-focus').focus();window.webEntry=await import('/web-entry.js');await webEntry.ensureAuthenticated();},theme);
    }
    await load();
    await page.locator('#natives-connection-notice').waitFor();
    assert.equal(await page.locator('dialog,input,form').count(),0,'no pairing workflow or modal may appear');
    assert.equal(await page.locator('#workspace-focus').evaluate(el=>el===document.activeElement),true,'notice must not steal focus');
    assert.match(await page.locator('#natives-connection-notice').innerText(),language==='en-US'?/Open this page from Natives/:/请从 Natives/);
    await page.locator('#natives-connection-notice button').focus();await page.keyboard.press('Enter');
    await page.waitForFunction(()=>!document.querySelector('#natives-connection-notice button').disabled);
    if(theme==='volt'&&language==='en-US'&&width===320)await page.screenshot({path:resolve(scratch,'connection-notice.png')});
    const rejected=await page.evaluate(async()=>{const r=await fetch('/web/session/exchange',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({pairingCode:'0123456789'})});return r.status;});
    assert.equal(rejected,401);
    await load(`#token=${await bootstrap()}`);
    const state=await page.evaluate(async()=>({authenticated:(await(await fetch('/web/session')).json()).authenticated,notice:document.querySelector('#natives-connection-notice')?.innerText,fragmentPresent:Boolean(location.hash)}));
    assert.equal(await page.locator('#natives-connection-notice,dialog,input').count(),0,JSON.stringify({...state,completed:checks.length,exchanges,cookies:(await context.cookies()).map(c=>({name:c.name,httpOnly:c.httpOnly}))}));
    assert.equal(await page.evaluate(()=>location.hash),'','one-time token is cleared from the URL');
    const cookie=(await context.cookies()).find(c=>c.name==='natives_dev_session');
    assert.equal(cookie.httpOnly,true);assert.equal(cookie.sameSite,'Strict');
    await load();
    assert.equal(await page.evaluate(()=>webEntry.ensureAuthenticated()),true,'refresh reuses browser authorization');
    const session=await page.evaluate(async()=>{const r=await fetch('/web/session');return r.json();});
    assert.equal(session.authenticated,true);
    // Revocation remains enforced; a replacement trusted launch restores an already booted page.
    await page.evaluate(async()=>{await import('/platform/session.js').then(m=>m.revokeWebSession());window.dispatchEvent(new Event('natives-web-session-required'));});
    await page.locator('#natives-connection-notice').waitFor();
    assert.equal(await page.evaluate(()=>webEntry.ensureAuthenticated()),false,'revocation must remove browser authorization');
    // One shared Core allows 10 exchanges/minute; the extra live recovery needs one case.
    if(checks.length===0){
      await page.evaluate(async token=>{location.hash=`#token=${token}`;const session=await import('/platform/session.js');session.invalidateWebSession();await session.initWebSession();},await bootstrap());
      assert.equal(await page.evaluate(()=>webEntry.ensureAuthenticated()),true);
    }
    assert.equal(await page.locator('dialog').count(),0);
    assert.deepEqual(errors,[]);
    checks.push({theme,language,width,noPairing:true,nonModal:true,bootstrap:true,cookie:true,refresh:true,revoke:true});
    await context.close();
  }
  const invalid=()=>fetch(`${base}/web/session/exchange`,{method:'POST',headers:{'Origin':base,'Content-Type':'application/json'},body:JSON.stringify({bootstrapToken:'invalid'})}).then(r=>r.json());
  assert.equal((await invalid()).error,'ERR_BOOTSTRAP_INVALID');
  assert.equal((await invalid()).error,'ERR_RATE_LIMIT_EXCEEDED','existing exchange rate limit remains enforced');
  writeFileSync(resolve(scratch,'browser-report.json'),JSON.stringify(checks,null,2));
  console.log(`No-pairing Chrome/Core checks passed: ${checks.length} layouts; trusted launch, Cookie, refresh, revoke, non-modal retry, retired CLI.`);
} finally {
  if(browser)await browser.close();
  if(core.exitCode===null){const exited=once(core,'exit');core.kill('SIGTERM');await exited;}
  writeFileSync(resolve(out,'core.stderr.log'),stderr);
}
