// Real signed Fund + Core in an isolated home; normal Chrome visibility and real idle deadlines.
import assert from 'node:assert/strict';
import {spawn,spawnSync,execFileSync} from 'node:child_process';
import {mkdirSync,mkdtempSync,copyFileSync,readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve} from 'node:path';
import {randomBytes} from 'node:crypto';
import {homedir} from 'node:os';
import net from 'node:net';
import {chromium} from 'playwright-core';

const root=resolve(import.meta.dirname,'..'),scratch=resolve(root,'.tmp/web-idle-recovery');
mkdirSync(scratch,{recursive:true});
const out=mkdtempSync(resolve(scratch,'run-'));
const env={...process.env,HOME:resolve(out,'home'),RUSTUP_HOME:process.env.RUSTUP_HOME||resolve(homedir(),'.rustup'),CARGO_HOME:process.env.CARGO_HOME||resolve(homedir(),'.cargo'),NATIVES_DEV_PRODUCT_SOURCE:resolve(out,'product-source'),NATIVES_STATE_DIR:resolve(out,'state'),NATIVES_WORKSPACE_DB:resolve(out,'workspace.db'),NATIVES_APPS_ROOT:resolve(out,'apps'),NATIVES_WEB_ROOT:resolve(root,'extension')};
for(const dir of ['Desktop','Documents','Downloads'])mkdirSync(resolve(env.HOME,dir),{recursive:true});
const prepared=spawnSync('rtk',['proxy',process.execPath,'scripts/apps/refresh-dev-product-source.mjs'],{cwd:root,env,encoding:'utf8',timeout:120000});
assert.equal(prepared.status,0,prepared.stderr||prepared.stdout);
const binary=resolve(out,'native-file-host');copyFileSync(resolve(root,'target/debug/native-file-host'),binary);
const port=await new Promise(done=>{const s=net.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>done(p));});});
const base=`http://127.0.0.1:${port}`,token=randomBytes(32).toString('base64url');
const core=spawn(binary,['--service','--port',String(port),'--bootstrap',token],{env,stdio:['ignore','ignore','pipe']});
let stderr='',browser,chrome;core.stderr.on('data',c=>stderr+=c);
const report={out,layouts:[],steps:[],errors:[]};
const wait=ms=>new Promise(r=>setTimeout(r,ms));
const runtimePids=()=>{
  const rows=execFileSync('ps',['-axo','pid,ppid,command'],{encoding:'utf8'}).split('\n');
  return rows.filter(row=>{const [pid,parent]=row.trim().split(/\s+/);return Number(parent)===core.pid&&row.includes('natives-app-runtime');}).map(row=>Number(row.trim().split(/\s+/)[0]));
};
try{
  for(let i=0;;i++){
    try{if((await fetch(base+'/healthz')).ok)break;}catch{}
    assert.ok(i<100&&core.exitCode===null,stderr);await wait(100);
  }
  const profile=resolve(out,'chrome-profile');mkdirSync(profile);
  chrome=spawn(process.env.NATIVES_TEST_CHROME||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',['--headless=new','--no-startup-window','--remote-debugging-port=0',`--user-data-dir=${profile}`,'--no-first-run'],{env,stdio:['ignore','ignore','pipe']});
  chrome.stderr.on('data',()=>{});
  for(let i=0;!existsSync(resolve(profile,'DevToolsActivePort'));i++){assert.ok(i<100);await wait(100);}
  const debugPort=readFileSync(resolve(profile,'DevToolsActivePort'),'utf8').split('\n')[0];
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${debugPort}`,{noDefaults:true});
  // These empty documents isolate connection UI only; the lifecycle case below loads the real app.
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en-US'])for(const width of [1280,320]){
    const context=await browser.newContext({locale:language,viewport:{width,height:800}});
    await context.route('**/connection-check.html',route=>route.fulfill({contentType:'text/html',body:`<!doctype html><html data-theme="${theme}"><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/theme.css"><body><button id="focus">Workspace</button><script type="module" src="/web-entry.js"></script></body></html>`}));
    await context.route('**/_locales/**',route=>route.abort());
    const page=await context.newPage();await page.goto(base+'/connection-check.html');
    await page.locator('#focus').focus();await page.locator('#natives-connection-notice').waitFor();
    const text=await page.locator('#natives-connection-notice').innerText();
    assert.doesNotMatch(text,/webConnection/);assert.match(text,language==='zh-CN'?/请从 Natives/:/Open this page/);
    assert.equal(await page.locator('#focus').evaluate(el=>el===document.activeElement),true);
    await page.locator('#natives-connection-notice button').focus();await page.keyboard.press('Enter');
    await page.waitForFunction(()=>!document.querySelector('#natives-connection-notice button').disabled);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
    if(width===320)await page.screenshot({path:resolve(out,`${theme}-${language}-offline.png`)});
    report.layouts.push({theme,language,width,offlineCopy:true,keyboard:true});await context.close();
  }
  console.log('Offline connection copy: 8 real Chrome layouts passed.');
  const context=browser.contexts()[0],page=await context.newPage();
  page.on('pageerror',e=>report.errors.push(e.message));
  await page.goto(`${base}/app.html?app=fund#token=${token}`);await page.bringToFront();
  const module=()=>page.frames().find(frame=>frame!==page.mainFrame()&&frame.url().startsWith('http://127.0.0.1:'));
  await page.frameLocator('iframe').locator('#home-privacy-btn').waitFor({timeout:30000});
  const first=runtimePids();assert.equal(first.length,1);report.firstRuntime=first[0];
  const foreground=await context.newPage();await foreground.goto('about:blank');await foreground.bringToFront();
  await page.waitForFunction(()=>document.visibilityState==='hidden');
  const hiddenAt=Date.now();
  for(let step=0;step<7;step++){
    await wait(20000);assert.equal(core.exitCode,null,'open dormant document must keep its recovery connection');
    console.log(`Idle recovery: hidden ${Math.round((Date.now()-hiddenAt)/1000)} s, Runtime count ${runtimePids().length}.`);
  }
  assert.deepEqual(runtimePids(),[],'real Fund Worker must be reclaimed after its original idle deadlines');
  const health=await(await fetch(base+'/healthz')).json();assert.equal(health.activeSurfaces,0);
  report.steps.push({hiddenSeconds:Math.round((Date.now()-hiddenAt)/1000),businessWorkers:0,businessSurfaces:0,corePid:core.pid});
  await page.bringToFront();
  await page.waitForFunction(()=>document.visibilityState==='visible');
  await page.frameLocator('iframe').locator('#home-privacy-btn').waitFor({timeout:30000});
  const second=runtimePids();assert.equal(second.length,1);assert.notEqual(second[0],first[0]);
  assert.ok(module());assert.equal(await page.locator('#natives-connection-notice').count(),0);
  report.steps.push({automaticResume:true,newRuntime:second[0]});
  await page.close();await foreground.close();
  const closedAt=Date.now();
  while(core.exitCode===null&&Date.now()-closedAt<130000){await wait(1000);}
  assert.equal(core.exitCode,0,'last document and Worker leaving must still release Core');
  report.steps.push({lastDocumentClosed:true,coreExitSeconds:Math.round((Date.now()-closedAt)/1000)});
  assert.deepEqual(report.errors,[]);
  console.log('Real Fund idle → Worker exit → visible automatic restart → last-page Core exit passed.');
}finally{
  await browser?.close().catch(()=>{});if(chrome?.exitCode===null)chrome.kill('SIGTERM');
  if(core.exitCode===null)core.kill('SIGTERM');
  writeFileSync(resolve(out,'core.stderr.log'),stderr);writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2));
}
