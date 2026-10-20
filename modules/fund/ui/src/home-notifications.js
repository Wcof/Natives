// User-approved, minimal business notifications. Runtime owns transport and Secrets.
(() => {
const {home,api,t,state}=Fund;
const esc=home.escape;
let taskId=null,requestId=null,generation=0;
function cancel(){generation++;const target=taskId?{taskId}:requestId?{requestId}:null;taskId=null;requestId=null;if(target)api('POST','/api/notify/cancel',target).catch(()=>{});}
home.cancelBusinessNotification=cancel;
home.notifyBusiness=async(subject,subjectId,snapshotId)=>{
 cancel();let gen=generation;
 try{
  const {channels}=await api('GET','/api/notify/channels');
  const available=channels.filter(c=>c.enabled&&!c.archived);
  if(gen!==generation)return;
  if(!available.length){home.status(t('fundHomeNotifyNoDestination'));return;}
  let preview=null;
  const root=home.panel(()=>t('fundHomeNotifyTitle'),()=>`<p>${esc(t('fundHomeNotifyDisclosure'))}</p><form>${home.select('destination',t('fundHomeNotifyDestination'),available.map(c=>[c.id,c.name]),available.find(c=>c.isDefault)?.id||available[0].id)}<button type="submit">${esc(t('fundHomePreviewOutboundContent'))}</button></form><pre data-message tabindex="0"></pre><button data-business-send disabled>${esc(t('fundHomeNotifyConfirm'))}</button><p data-notify-result role="status"></p>`,root=>{
   const form=root.querySelector('form'),send=root.querySelector('[data-business-send]');
   form.elements.destination.onchange=()=>{preview=null;send.disabled=true;root.querySelector('[data-message]').textContent='';};
   home.submit(root,async v=>{
    const p=await api('POST','/api/notify/preview',{id:Number(v.destination),subject,subjectId,...(snapshotId?{snapshotId}:{}),language:state.language==='en'?'en':'zh_CN'});
    if(gen!==generation||!root.isConnected)return;
    preview=p;root.querySelector('[data-message]').textContent=`${p.channelName}\n${p.title}\n${p.body}${p.recipients?.length?'\n@ '+p.recipients.join(', '):''}\n${t('fundHomeNotifyEvidence')}: ${p.snapshotId}`;send.disabled=false;
   });
   send.onclick=async()=>{
    if(!preview)return;send.disabled=true;
    try{
     requestId=home.requestId();
     const started=await api('POST','/api/notify/send',{id:preview.id,subject,subjectId,...(snapshotId?{snapshotId}:{}),language:state.language==='en'?'en':'zh_CN',previewToken:preview.previewToken,requestId});
     if(gen!==generation||!root.isConnected){await api('POST','/api/notify/cancel',{taskId:started.taskId});return;}
     taskId=started.taskId;
     for(let i=0;i<300;i++){
      await new Promise(resolve=>setTimeout(resolve,250));
      if(gen!==generation||!root.isConnected)return;
      const result=await api('GET',`/api/notify/status?taskId=${taskId}`);
      if(result.status==='running')continue;
      taskId=null;requestId=null;
      const code=result.result?.status;
      root.querySelector('[data-notify-result]').textContent=t(code==='sent'?'fundHomeNotifySent':code==='skipped_silent'?'fundHomeNotifySilent':code==='skipped_dedup'?'fundHomeNotifyDedup':'fundHomeNotifyUnknown');
      return;
     }
     cancel();root.querySelector('[data-notify-result]').textContent=t('fundHomeNotifyUnknown');
    }catch{root.querySelector('[data-notify-result]').textContent=t('fundHomeNotifyUnknown');}
   };
  },true);
  gen=generation;if(root)root.dataset.businessNotification="true";
 }catch{home.status(t('fundHomeNotifyUnknown'));}
};
const close=home.closePanel;
home.closePanel=()=>{cancel();close();};
document.addEventListener('cancel',cancel,true);
document.addEventListener('click',e=>{if(e.target.closest?.('.home-manager [data-close]'))cancel();});
document.addEventListener('visibilitychange',()=>{if(document.hidden)cancel();});
document.addEventListener('fund:language-changed',()=>{if(document.querySelector('[data-business-notification]'))home.closePanel();});
window.addEventListener('pagehide',cancel);
})();
