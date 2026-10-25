// One detail dialog owns its reads; closing it cannot reopen from a late response.
(() => {
const {home,api,t}=globalThis.Fund;
const esc=home.escape;
const externalUrl=value=>{try{const url=new URL(String(value));return ['http:','https:'].includes(url.protocol)?url.href:'';}catch{return '';}};
let active=null;
home.openInformationDetail=async id=>{
  const controller=new AbortController();
  let detail=null,error=null,linkError='',loading=true,fetching=false,root;
  const alive=()=>root?.isConnected&&!controller.signal.aborted;
  const message=()=>linkError||(error?`${t('fundHomeInformationCouldNotLoad')}${Fund.localizeApiError(error.status||500,error.code||'INFORMATION_UPSTREAM','')}`:'');
  const supported=()=>detail?.contentSupported===true;
  const markup=()=>{
    if(!detail)return `<p role="status">${esc(t(loading?'fundHomeLoading':'fundHomeInformationCouldNotLoad'))}</p>${loading?'':`<button type="button" data-info-retry>${esc(t('fundHomeInformationRetry'))}</button>`}`;
    const d=detail;
    return `<p><b>${esc(d.title)}</b></p><p class="evidence-hint">${esc(home.informationSourceLabel(d.sourceId))} · ${esc(home.informationTimeLabel(d.publishedAt,d.timePrecision))}</p>
      ${d.summary?`<p>${esc(d.summary)}</p>`:''}
      ${d.contentText?`<details open><summary>${esc(home.informationContentLabel(d))}</summary><pre class="information-content-text">${esc(d.contentText)}</pre></details>`:`<p class="evidence-hint">${esc(t(d.completeness==='unavailable'?'fundHomeInformationContentUnavailable':'fundHomeContentNotRetrieved'))}</p>`}
      ${d.contentCacheExpired?`<p class="goal-warn">${esc(t('fundHomeInformationContentExpired'))}</p>`:''}
      ${fetching?`<p role="status">${esc(t('fundHomeInformationContentLoading'))}</p>`:''}
      ${supported()?`<button type="button" data-info-fetch="${id}" ${fetching?'disabled':''}>${esc(t('fundHomeInformationFetchContent'))}</button>`:''}
      ${d.url&&externalUrl(d.url)?`<p><button type="button" class="btn btn-sm" data-info-url="${esc(externalUrl(d.url))}">${esc(t('fundHomeOpenSource'))}</button></p>`:''}
      ${(d.relations||[]).length?`<p class="evidence-hint">${esc(t('fundHomeInformationRelations'))}<br>${d.relations.map(r=>`${esc(r.instrumentRef)} · ${esc(t(r.relation==='direct'?'fundHomeInformationDirect':r.relation==='indirect'?'fundHomeInformationIndirect':'fundHomeNotConfirmed'))}：${esc(r.reason)}`).join('<br>')}</p>`:''}
      ${d.relatedItems?.length?`<h3>${esc(t('fundHomeInformationPossibleRelated'))}</h3><p>${esc(t('fundHomeInformationPossibleRelatedHint'))}</p>${d.relatedItems.map(item=>`<p><button type="button" data-related-info="${item.id}">${esc(item.title)}</button> · ${esc(home.informationSourceLabel(item.sourceId))}</p>`).join('')}`:''}
      <button type="button" data-detail-ai>${esc(t('fundHomeNewsAiBtn'))}</button>`;
  };
  const paint=()=>{
    if(!alive())return;
    const focus=root.contains(document.activeElement)?document.activeElement:null;
    const action=focus?.hasAttribute('data-info-fetch')?'[data-info-fetch]':focus?.hasAttribute('data-info-retry')?'[data-info-retry]':null;
    root.querySelector('.home-manager-body').innerHTML=markup();bind(root);
    if(action)(root.querySelector(action)||root.querySelector('[data-close]'))?.focus();
  };
  async function fetchContent(){
    if(!alive()||fetching||!supported())return;
    fetching=true;error=null;paint();
    try{
      const result=await home.requestInformation('/api/information/content',{id,expectedVersion:detail.contentVersion});
      if(!result||!alive())return;
      const next=await api('GET',`/api/information/item?id=${id}`,null,{signal:controller.signal});
      if(alive())detail=next;
    }catch(e){if(alive())error=e;}
    finally{fetching=false;paint();}
  }
  async function read(){
    loading=true;error=null;paint();
    try{
      const next=await api('GET',`/api/information/item?id=${id}`,null,{signal:controller.signal});
      if(!alive())return;
      detail=next;loading=false;paint();
      if(supported()&&!detail.contentText&&!detail.contentFetchedAt)await fetchContent();
    }catch(e){if(alive()){loading=false;error=e;paint();}}
  }
  function bind(node){
    node.querySelector('[data-info-retry]')?.addEventListener('click',read);
    node.querySelector('[data-info-fetch]')?.addEventListener('click',fetchContent);
    node.querySelector('[data-info-url]')?.addEventListener('click',async e=>{
      const button=e.currentTarget;
      if(button.disabled)return;
      e.preventDefault();linkError='';button.disabled=true;
      try{await home.openExternal(button.dataset.infoUrl);}
      catch{if(alive())linkError=t('fundHomeExternalLinkFailed');}
      finally{if(button.isConnected)button.disabled=false;if(alive())home.failure({message:message()},root);}
    });
    node.querySelectorAll('[data-related-info]').forEach(b=>b.onclick=()=>home.openInformationDetail(Number(b.dataset.relatedInfo)));
    node.querySelector('[data-detail-ai]')?.addEventListener('click',()=>{const d=detail;home.closePanel();document.getElementById('tab-home').click();home.analyzeNewsForAI?.(d);});
    home.failure({message:message()},node);
  }
  active?.abort();
  root=home.panel(()=>t('fundHomeInformationDetail'),markup,bind,true);
  if(!root)return;
  active=controller;
  root.addEventListener('close',()=>{controller.abort();if(active===controller)active=null;},{once:true});
  await read();
};
window.addEventListener('pagehide',()=>{active?.abort();active=null;});
})();
