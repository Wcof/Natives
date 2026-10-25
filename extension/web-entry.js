// Web startup uses trusted-launch authorization; there is no pairing workflow.
import { isExtension } from './platform/env.js';
import { initWebSession, invalidateWebSession } from './platform/session.js';
import { initI18n, getMessage } from './platform/i18n.js';

let pageAuthenticated=false,noticeGeneration=0;
function clearNotice(){noticeGeneration++;document.getElementById('natives-connection-notice')?.remove();}
function sessionRestored(){
  const wasBlocked=Boolean(document.getElementById('natives-connection-notice'));
  clearNotice();
  if(wasBlocked&&!pageAuthenticated)window.location.reload();
}
async function showConnectionNotice(){
  if(isExtension||typeof document==='undefined')return;
  const generation=noticeGeneration;
  await initI18n();
  if(generation!==noticeGeneration)return;
  if(document.getElementById('natives-connection-notice'))return;
  const notice=document.createElement('section');
  notice.id='natives-connection-notice';notice.setAttribute('role','status');notice.setAttribute('aria-live','polite');
  notice.style.cssText='position:fixed;bottom:16px;left:16px;right:16px;z-index:999999;padding:16px;border:1px solid var(--natives-border);border-radius:10px;background:var(--natives-surface);color:var(--natives-text-primary);font:14px var(--natives-font);';
  const title=document.createElement('strong');title.textContent=getMessage('webConnectionRequiredTitle');
  const description=document.createElement('p');description.textContent=getMessage('webConnectionRequiredDescription');
  const retry=document.createElement('button');retry.type='button';retry.textContent=getMessage('webConnectionRetry');
  retry.style.cssText='min-height:36px;padding:8px 14px;border:1px solid var(--natives-control);border-radius:8px;background:var(--natives-surface-2);color:var(--natives-text-primary);font:inherit;cursor:pointer;';
  retry.addEventListener('click',async()=>{
    retry.disabled=true;
    try{
      invalidateWebSession();
      const session=await initWebSession();
      if(session.authenticated)window.dispatchEvent(new Event('natives-web-session-restored'));
      else description.textContent=getMessage('webConnectionRequiredDescription');
    }catch{description.textContent=getMessage('webConnectionRequiredDescription');}
    finally{if(retry.isConnected)retry.disabled=false;}
  });
  notice.append(title,description,retry);document.body.append(notice);
}

export async function ensureAuthenticated(){
  if(isExtension)return true;
  const session=await initWebSession();
  if(session.authenticated){pageAuthenticated=true;clearNotice();return true;}
  await showConnectionNotice();return false;
}

if(typeof window!=='undefined'&&!isExtension){
  window.addEventListener('natives-web-session-required',showConnectionNotice);
  window.addEventListener('natives-web-session-restored',sessionRestored);
  window.addEventListener('natives-web-surface',event=>{
    if(event.detail?.state==='error')void showConnectionNotice();
    else if(event.detail?.state==='ready')clearNotice();
  });
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',()=>{void ensureAuthenticated();},{once:true});
  else void ensureAuthenticated();
}
