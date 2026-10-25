import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

globalThis.document = Object.assign(new EventTarget(), { visibilityState:'visible' });
globalThis.window = new EventTarget();
globalThis.location = {origin:'http://127.0.0.1:8771',hash:''};
globalThis.sessionStorage = {setItem(){}};
const streams=[],requests=[];
globalThis.EventSource=class {
  constructor(url){this.url=url;streams.push(this);}
  close(){this.closed=true;}
};
globalThis.fetch=async(url,options={})=>{
  const path=new URL(url,location.origin).pathname;
  requests.push({path,method:options.method});
  if(path.startsWith('/_locales/'))throw TypeError('offline');
  return {ok:true,json:async()=>path==='/web/session'?{authenticated:true,csrfToken:'isolated'}:{surfaceId:'surface-'+requests.length}};
};
const i18n=await import('./i18n.js');
for(const locale of ['zh_CN','en']){
  await i18n.initI18n(locale);
  const messages=JSON.parse(readFileSync(new URL(`../_locales/${locale}/messages.json`,import.meta.url)));
  for(const key of ['webConnectionRequiredTitle','webConnectionRequiredDescription','webConnectionRetry']){
    assert.equal(i18n.getMessage(key),messages[key].message,'offline connection copy must be readable and match the locale authority');
  }
}
globalThis.fetch=async()=>({ok:true,json:async()=>({})});
await i18n.initI18n('zh_CN');
assert.equal(i18n.getMessage('webConnectionRetry'),'重试连接','an old language file missing the key uses the same generated copy');
globalThis.fetch=async(url,{signal})=>new Promise((_,reject)=>signal.addEventListener('abort',()=>reject(signal.reason),{once:true}));
const began=Date.now();
const keepTestAlive=setTimeout(()=>{},3000);
await i18n.initI18n('en');
clearTimeout(keepTestAlive);
assert.ok(Date.now()-began<3000,'a stalled language server cannot hang the notice');
assert.equal(i18n.getMessage('webConnectionRetry'),'Retry connection');
globalThis.fetch=async(url,options={})=>{
  const path=new URL(url,location.origin).pathname;requests.push({path,method:options.method});
  return {ok:true,json:async()=>path==='/web/session'?{authenticated:true,csrfToken:'isolated'}:{surfaceId:'surface-'+requests.length}};
};
const {subscribeCoreEvents}=await import('./events.js');
const {ensureWebSurface}=await import('./surface.js');
const received=[];
const dispose=subscribeCoreEvents(event=>received.push(event));
const flush=()=>new Promise(resolve=>setImmediate(resolve));
// Web session assets load through a dynamic import (they are excluded from the
// extension package), so the first relay needs more than a fixed number of
// microtask turns. Wait for the observable stream, then keep using flush below.
for(let attempt=0;attempt<50&&!streams.length;attempt++)await flush();
assert.equal(streams.length,1);
assert.match(streams[0].url,/surface=/);
document.visibilityState='hidden';document.dispatchEvent(new Event('visibilitychange'));
await flush();await flush();
assert.equal(streams[0].closed,true);
assert.equal(streams.length,2,'hidden document replaces its scoped stream with one dormant connection');
assert.equal(new URL(streams[1].url).search,'','dormant connection must not own a business Surface');
const before=received.length;
streams[1].onmessage({data:JSON.stringify({type:'workspace_changed',domain:'workspace',epoch:'one',sequence:4})});
assert.equal(received.length,before,'hidden events cannot trigger business queries');
dispose();
assert.ok(!streams[1].closed,'business clients can disconnect without dropping the document recovery connection');
streams[1].onerror();
assert.equal(received.length,before,'hidden disconnects cannot trigger business resync');
await flush();assert.equal(streams.length,2,'a hidden failure does not start reconnect polling');
window.dispatchEvent(new Event('pagehide'));
assert.equal(streams[1].closed,true);
const afterClose=streams.length;
document.visibilityState='visible';document.dispatchEvent(new Event('visibilitychange'));
await flush();assert.equal(streams.length,afterClose,'pagehide prevents visibility from reopening a released document');
window.dispatchEvent(Object.assign(new Event('pageshow'),{persisted:true}));
await flush();await flush();
assert.equal(streams.length,3,'BFCache restores one shared scoped stream');
const postCount=requests.filter(r=>r.path==='/web/surfaces'&&r.method==='POST').length;
assert.ok(postCount>=2,'return acquires a fresh business Surface after release');
await ensureWebSurface();
dispose();window.dispatchEvent(new Event('pagehide'));
assert.equal(streams.at(-1).closed,true);
console.log('Offline locale fallback, dormant document ownership, hidden suppression and BFCache recovery passed');
