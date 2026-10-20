// Shared display preferences are Core-owned. Browser storage keeps only local projections.
import { isExtension } from './env.js';
import { createPlatformClient } from './transport.js';
import { subscribeCoreEvents } from './events.js';
import { getMessage } from './i18n.js';

export const sharedDefaults = Object.freeze({
  'natives-theme':'archive', 'natives-language':'zh_CN', 'natives-view-mode':'list',
  'natives-grid-size':'medium', 'natives-sort-by':'name', 'natives-sort-direction':'asc',
  'natives-show-hidden':false, 'natives-follow-changes':false, 'natives-preview-width':360,
  'natives-preview-height':320, 'natives-preview-bottom':false, 'natives-sidebar-width':248,
  'natives-sidebar-collapsed':false, 'natives-recursive-search':false,
  'natives-inspector-width':360, 'natives-apps-sidebar-collapsed':false,
});
const shared = key => Object.hasOwn(sharedDefaults,key);
const listeners = new Set();
let client, snapshot, loading, queued = Promise.resolve(), dirty = 0, hiddenTimer, unsubscribe;
const parse = text => { try { return JSON.parse(text); } catch { return text ?? undefined; } };
async function browserStorage(operation) {
  let timer;
  try { return await Promise.race([operation(),new Promise((_,reject)=>{
    timer=setTimeout(()=>reject(Object.assign(Error('Browser storage timed out'),{code:'storage_timeout'})),500);
  })]); } finally {clearTimeout(timer);}
}
function emit(changes) {
  if (Object.keys(changes).length) for (const fn of listeners) {
    try { Promise.resolve(fn(changes,'local')).catch(reportError); } catch (error) { reportError(error); }
  }
}
function accept(next) {
  if (snapshot && next.revision < snapshot.revision) return;
  const changes = {};
  for (const key of Object.keys(sharedDefaults)) {
    const oldValue = snapshot?.entries[key] ?? sharedDefaults[key];
    const newValue = next.entries[key] ?? sharedDefaults[key];
    if (oldValue !== newValue) changes[key] = {oldValue,newValue};
  }
  snapshot = next; emit(changes);
  document.getElementById?.('shared-preference-error')?.remove();
}
function invalidate() {
  dirty++;
  if (document.visibilityState !== 'hidden') void readSnapshot(true).catch(reportError);
}
function ensureClient() {
  if (!client) client = createPlatformClient({host:'com.natives.file_manager'});
  if (!unsubscribe) unsubscribe = subscribeCoreEvents(event => {
    if (event.type === 'preferences_changed' || (event.type === 'resync' && event.reason !== 'unavailable')) invalidate();
  }, ['preferences']);
  return client;
}
export async function readSnapshot(force = false) {
  if (loading) return loading;
  if (snapshot && !force && !dirty) return snapshot;
  const revision = dirty;
  loading = ensureClient().call('preferences_get').then(next => { accept(next); if (dirty === revision) dirty = 0; return next; });
  try { return await loading; }
  finally { loading = undefined; if (dirty && dirty !== revision && document.visibilityState !== 'hidden') void readSnapshot(true).catch(reportError); }
}
function reportError(error) {
  window.dispatchEvent(new CustomEvent('natives-preferences-error',{detail:{error}}));
  if (!document.body) return;
  let message=document.getElementById('shared-preference-error');
  if (!message) {
    message=document.createElement('p'); message.id='shared-preference-error';
    message.className='shared-preference-error'; message.setAttribute('role','alert'); document.body.append(message);
  }
  message.textContent=getMessage('sharedPreferencesFailed');
}
async function localGet(keys) {
  if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) && chrome.storage?.local) return browserStorage(()=>chrome.storage.local.get(keys));
  if (typeof localStorage === 'undefined') return {};
  const names = typeof keys === 'string' ? [keys] : Array.isArray(keys) ? keys : keys ? Object.keys(keys) : Object.keys(localStorage);
  return Object.fromEntries(names.map(key => [key,localStorage.getItem(key) === null ? keys?.[key] : parse(localStorage.getItem(key))]));
}
async function get(keys) {
  const names = typeof keys === 'string' ? [keys] : Array.isArray(keys) ? keys : keys ? Object.keys(keys) : null;
  const localKeys=names?.filter(key=>!shared(key));
  const result=localKeys?.length===0 ? {} : await localGet(localKeys || keys);
  if (keys && !Array.isArray(keys) && typeof keys==='object') {
    for (const [key,value] of Object.entries(keys)) if(!shared(key) && result[key]===undefined)result[key]=value;
  }
  if (!names || names.some(shared)) {
    const current = await readSnapshot();
    for (const key of names || Object.keys(sharedDefaults)) if (shared(key)) result[key] = current.entries[key] ?? sharedDefaults[key];
  }
  return result;
}
function write(entries, source) {
  const operation = queued.then(async () => {
    const current = await readSnapshot();
    try {
      const next = await ensureClient().call(source ? 'preferences_import' : 'preferences_set',{
        entries,expectedRevision:current.revision,...(source ? {source} : {}),
      });
      accept(next); return next;
    } catch (error) { dirty++; reportError(error); throw error; }
  });
  queued = operation.catch(() => {});
  return operation;
}
async function set(items) {
  const entries = Object.fromEntries(Object.entries(items).filter(([key]) => shared(key)));
  if (Object.keys(entries).length) await write(entries);
  const local = Object.fromEntries(Object.entries(items).filter(([key]) => !shared(key)));
  if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) { if (Object.keys(local).length) await browserStorage(()=>chrome.storage.local.set(local)); }
  else {
    const old = await localGet(Object.keys(local));
    for (const [key,value] of Object.entries(local)) localStorage.setItem(key,JSON.stringify(value));
    emit(Object.fromEntries(Object.entries(local).map(([key,newValue]) => [key,{oldValue:old[key],newValue}])));
  }
}
async function remove(keys) {
  const names = Array.isArray(keys) ? keys : [keys];
  const entries = Object.fromEntries(names.filter(shared).map(key => [key,null]));
  if (Object.keys(entries).length) await write(entries);
  const local = names.filter(key => !shared(key));
  if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) await browserStorage(()=>chrome.storage.local.remove(local));
  else { const old = await localGet(local); for (const key of local) localStorage.removeItem(key); emit(Object.fromEntries(local.map(key => [key,{oldValue:old[key],newValue:undefined}]))); }
}
export async function previewLegacyPreferences() {
  if (!(typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) throw Error('EXTENSION_PROFILE_REQUIRED');
  const legacy = await browserStorage(()=>chrome.storage.local.get(Object.keys(sharedDefaults)));
  return {legacy,current:await readSnapshot(true)};
}
export const importLegacyPreferences = entries => write(entries,'selected-extension-profile/v1');
export const storage = {local:{get,set,remove},onChanged:{addListener:fn=>listeners.add(fn),removeListener:fn=>listeners.delete(fn)}};
if (typeof window !== 'undefined') {
  chromeStorageListener();
  if (!(typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) window.addEventListener('storage',event=>{if(event.key && !shared(event.key)) emit({[event.key]:{oldValue:parse(event.oldValue),newValue:parse(event.newValue)}});});
  const release = () => { clearTimeout(hiddenTimer); client?.disconnect(); unsubscribe?.(); unsubscribe=undefined; };
  window.addEventListener('pagehide',release);
  window.addEventListener('pageshow',event=>{if(event.persisted)invalidate();});
  document.addEventListener('visibilitychange',()=>{
    clearTimeout(hiddenTimer);
    if(document.visibilityState==='hidden') hiddenTimer=setTimeout(release,60_000);
    else if(snapshot) invalidate();
  });
}
function chromeStorageListener() {
  if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) chrome.storage?.onChanged?.addListener((changes,area)=>{
    if (area==='local') emit(Object.fromEntries(Object.entries(changes).filter(([key])=>!shared(key))));
  });
}
