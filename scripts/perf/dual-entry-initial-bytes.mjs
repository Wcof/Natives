// ADR-0042: measure the initial JS/CSS gzip for each entry page separately.
// The extension distributable estimate is NOT used as the initial-load metric.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { gzipSync } from 'node:zlib';
import { chromium } from 'playwright-core';
const root=resolve(process.env.NATIVES_ACCEPTANCE_ROOT||'.tmp/dual-entry-counts');const report=resolve(process.argv[2]||'.tmp/initial-bytes');mkdirSync(report,{recursive:true});
assert.ok(existsSync(root+'/product-source'),`missing ${root}/product-source`);
const state=mkdtempSync(tmpdir()+'/natives-bytes-');const home=state+'/home',profile=state+'/extension-profile',source=state+'/product-source';cpSync(root+'/product-source',source,{recursive:true});
copyFileSync(resolve('target/debug/native-file-host'),state+'/native-file-host');chmodSync(state+'/native-file-host',0o755);
for(const d of [home,profile,home+'/Desktop',home+'/Documents',home+'/Downloads'])mkdirSync(d,{recursive:true});
const port=await new Promise(done=>{const s=net.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>done(p));});});
const base=`http://127.0.0.1:${port}`,token=randomBytes(32).toString('base64url');
const env={...process.env,HOME:home,NATIVES_WORKSPACE_DB:state+'/workspace.db',NATIVES_APPS_ROOT:state+'/apps',NATIVES_STATE_DIR:state+'/state',NATIVES_DEV_PRODUCT_SOURCE:source,NATIVES_WEB_PORT:String(port)};
const q=v=>"'"+v.replaceAll("'","'\"'\"'")+"'";const wrapper=state+'/native.sh';
writeFileSync(wrapper,`#!/bin/sh\nexec /usr/bin/env HOME=${q(home)} NATIVES_WORKSPACE_DB=${q(state+'/workspace.db')} NATIVES_APPS_ROOT=${q(state+'/apps')} NATIVES_STATE_DIR=${q(state+'/state')} NATIVES_DEV_PRODUCT_SOURCE=${q(source)} NATIVES_WEB_PORT=${port} ${q(resolve('target/debug/native-file-host'))} "$@"\n`);chmodSync(wrapper,0o755);
for(const d of [home+'/Library/Application Support/Google/Chrome/NativeMessagingHosts',home+'/Library/Application Support/Google/Chrome for Testing/NativeMessagingHosts',profile+'/NativeMessagingHosts']){mkdirSync(d,{recursive:true});writeFileSync(d+'/com.natives.local.file_manager.json',JSON.stringify({name:'com.natives.local.file_manager',description:'initial bytes',path:wrapper,type:'stdio',allowed_origins:['chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/']}));}
const service=spawn(resolve('target/debug/native-file-host'),['--service','--port',String(port),'--bootstrap',token],{env,stdio:['ignore','ignore','pipe']});let stderr='';service.stderr.on('data',x=>stderr+=x);
let browser,ext;
const gzipSum=async page=>{const rows=[];const seen=new Set();const onResponse=async res=>{const url=res.url();if(!/\.(js|css)(\?|$)/.test(url)||seen.has(url))return;seen.add(url);try{const body=await res.body();if(body.length)rows.push({url,bytes:body.length,gzip:gzipSync(body).length,status:res.status()});}catch{}};
 page.on('response',onResponse);return {rows,finish:async()=>{await page.waitForTimeout(1500);page.off('response',onResponse);rows.sort((a,b)=>b.gzip-a.gzip);return {files:rows.length,raw:rows.reduce((s,r)=>s+r.bytes,0),gzip:rows.reduce((s,r)=>s+r.gzip,0),rows};}};};
try{
 for(let i=0;i<100;i++){try{if((await fetch(base+'/healthz')).ok)break;}catch{}await new Promise(r=>setTimeout(r,50));}
 browser=await chromium.launch({executablePath:process.env.CHROME_EXECUTABLE,headless:true});
 const web=await browser.newContext({viewport:{width:1440,height:1000}});const w=await web.newPage();const werr=[];w.on('pageerror',e=>werr.push(e.message));
 await w.goto(`${base}/space.html#token=${token}`,{waitUntil:'load'});await w.waitForFunction(async()=> (await(await fetch('/web/session')).json()).authenticated);
 ext=await chromium.launchPersistentContext(profile,{executablePath:process.env.CHROME_EXECUTABLE,headless:true,env,viewport:{width:1440,height:1000},ignoreDefaultArgs:['--disable-extensions'],args:[`--disable-extensions-except=${source}/ChromeExtension`,`--load-extension=${source}/ChromeExtension`]});const e=await ext.newPage();const eerr=[];e.on('pageerror',x=>eerr.push(x.message));
 const results={};
 for(const [entry,page,makeUrl] of [['E',e,p=>`chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/${p}`],['W',w,p=>`${base}/${p}`]]){
  results[entry]={};
  for(const pageName of ['space.html','files.html']){
   const probe=await gzipSum(page);await page.goto(makeUrl(pageName),{waitUntil:'load'});await page.waitForTimeout(1000);const data=await probe.finish();results[entry][pageName]=data;
   if(pageName==='files.html')await page.locator('#entries').waitFor({timeout:15000}).catch(()=>{});
  }
 }
 const output={scope:'real production entry pages, fresh isolated profiles; summed JS/CSS response bodies gzipped independently',results,errors:{E:eerr,W:werr},stderr:stderr.slice(-300)};
 writeFileSync(report+'/initial-bytes.json',JSON.stringify(output,null,2));console.log(JSON.stringify({E:Object.fromEntries(Object.entries(results.E).map(([k,v])=>[k,{gzip:v.gzip,raw:v.raw,files:v.files}])),W:Object.fromEntries(Object.entries(results.W).map(([k,v])=>[k,{gzip:v.gzip,raw:v.raw,files:v.files}]))},null,2));
}finally{await ext?.close();await browser?.close();service.kill('SIGTERM');}
