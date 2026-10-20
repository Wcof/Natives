// ADR-0042 measurement: real SQL/DB counters with NATIVES_SQL_TRACE=1.
// Counts only; no SQL text, parameters, paths or business values are exposed.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { chromium } from 'playwright-core';
const root=resolve(process.env.NATIVES_ACCEPTANCE_ROOT||'.tmp/dual-entry-counts');const report=resolve(process.argv[2]||'.tmp/sql-counts');mkdirSync(report,{recursive:true});
assert.ok(existsSync(root+'/product-source'),`missing ${root}/product-source`);
const state=mkdtempSync(tmpdir()+'/natives-sql-');const home=state+'/home',source=state+'/product-source';cpSync(root+'/product-source',source,{recursive:true});copyFileSync(resolve('target/debug/native-file-host'),state+'/native-file-host');chmodSync(state+'/native-file-host',0o755);for(const d of [home,home+'/Desktop',home+'/Documents',home+'/Downloads'])mkdirSync(d,{recursive:true});
const port=await new Promise(done=>{const s=net.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>done(p));});});
const base=`http://127.0.0.1:${port}`,token=randomBytes(32).toString('base64url');
const env={...process.env,HOME:home,NATIVES_WORKSPACE_DB:state+'/workspace.db',NATIVES_APPS_ROOT:state+'/apps',NATIVES_STATE_DIR:state+'/state',NATIVES_DEV_PRODUCT_SOURCE:source,NATIVES_WEB_PORT:String(port),NATIVES_SQL_TRACE:'1'};
const service=spawn(resolve('target/debug/native-file-host'),['--service','--port',String(port),'--bootstrap',token],{env,stdio:['ignore','ignore','pipe']});let stderr='';service.stderr.on('data',x=>stderr+=x);
let browser;
try{
 for(let i=0;i<100;i++){try{if((await fetch(base+'/healthz')).ok)break;}catch{}await new Promise(r=>setTimeout(r,50));}
 browser=await chromium.launch({executablePath:process.env.CHROME_EXECUTABLE,headless:true});const ctx=await browser.newContext({viewport:{width:1440,height:1000}});const w=await ctx.newPage(),f=await ctx.newPage();const errs=[];for(const p of [w,f])p.on('pageerror',e=>errs.push(e.message));
 await w.goto(`${base}/space.html#token=${token}`,{waitUntil:'load'});await w.waitForFunction(async()=> (await(await fetch('/web/session')).json()).authenticated);
 await f.goto(`${base}/app.html?app=fund#token=${token}`,{waitUntil:'load'});await f.waitForFunction(async()=> (await(await fetch('/web/session')).json()).authenticated);await f.locator('iframe').waitFor({timeout:20000});const fundFrame=f.frames().find(x=>x!==f.mainFrame()&&x.url().startsWith('http://127.0.0.1:'));await fundFrame.locator('#view-home').waitFor({state:'visible',timeout:20000});
 async function core(method){return w.evaluate(async method=>{const {createPlatformClient}=await import('./platform/transport.js');const c=createPlatformClient({host:'com.natives.local.file_manager'});try{return await c.call(method,{});}finally{c.disconnect();}},method);}
 async function fund(method){return fundFrame.evaluate(async method=>Fund.api(method,'/api/diagnostics/sql'),method);}
 async function seed(n){return w.evaluate(async n=>{const {createPlatformClient}=await import('./platform/transport.js');const {widgetPlugins}=await import('./plugins/widgets/index.js');const {INVESTMENT_WIDGET_KEYS}=await import('./space-investment-feed.js');const c=createPlatformClient({host:'com.natives.local.file_manager'});try{const {workspace}=await c.call('workspace_create',{name:`sql-${n}`,template:'blank'});let s=await c.call('workspace_snapshot',{workspaceId:workspace.id});for(let i=0;i<n;i++){const key=INVESTMENT_WIDGET_KEYS[i%INVESTMENT_WIDGET_KEYS.length];const d=widgetPlugins[key];s=await c.call('workspace_widget_upsert',{workspaceId:workspace.id,expectedRevision:s.revision,widget:{id:'',key,enabled:true,order:s.widgets.length,configJson:d.defaultData||{},displayJson:{position:'middleCentre'}}});}return workspace.id;}finally{c.disconnect();}},n);}
 const rows=[];
 for(const n of [1,5,20]){const id=await seed(n);await fund('POST');const cr=await core('diagnostics_reset');const started=Date.now();await w.goto(`${base}/space.html?workspace=${id}`,{waitUntil:'load'});await w.locator('.Widget').nth(n-1).waitFor({timeout:30000});await w.waitForTimeout(1200);const fs=await fund('GET');const cs=await core('diagnostics_get');rows.push({cards:n,elapsedMs:Date.now()-started,core:cs,fund:fs});}
 const output={scope:'real web entry, isolated Core/Fund, NATIVES_SQL_TRACE=1; counts only',rows,errors:errs,stderr:stderr.slice(-300)};writeFileSync(report+'/sql-counts.json',JSON.stringify(output,null,2));console.log(JSON.stringify(output,null,2));
 assert.equal(rows.length,3);
}finally{await browser?.close();service.kill('SIGTERM');}
