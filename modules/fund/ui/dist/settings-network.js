// Module transport settings; proxy credentials stay in the trusted Shell.
(() => {
const {api,home,t,actions}=Fund;
const esc=home.escape;
home.manageNetworkSettings=async()=>{
 try{
  const config=await api('GET','/api/settings/network');let probeToken=null,signature=null;
  home.panel(()=>t('fundHomeNetworkTitle'),()=>`<p>${esc(t('fundHomeNetworkBoundary'))}</p><form>${home.select('mode',t('fundHomeNetworkMode'),[['direct',t('fundHomeNetworkDirect')],['https_proxy',t('fundHomeNetworkProxy')]],config.mode)}${home.input('url',t('fundHomeNetworkUrl'),config.url,'url','maxlength="512"')}${home.input('username',t('fundHomeNetworkUsername'),config.username,'text','maxlength="128"')}<button type="button" data-proxy-secret>${esc(t('fundHomeSettingsConfigure'))}</button><button type="button" data-proxy-delete>${esc(t('fundHomeSettingsRemoveCredential'))}</button><button type="button" data-proxy-probe>${esc(t('fundHomeNetworkProbe'))}</button><button type="submit">${esc(t('fundHomeSave'))}</button><p data-network-state role="status"></p></form>`,root=>{
   const form=root.querySelector('form'),status=root.querySelector('[data-network-state]');let requestId=home.requestId();
   const value=()=>form.elements.mode.value==='direct'?{mode:'direct',url:'',username:''}:{mode:'https_proxy',url:form.elements.url.value.trim(),username:form.elements.username.value};
   const reset=()=>{probeToken=null;signature=null;requestId=home.requestId();status.textContent='';const proxy=form.elements.mode.value==='https_proxy';for(const name of ['url','username'])form.elements[name].disabled=!proxy;root.querySelector('[data-proxy-probe]').disabled=!proxy;};
   form.oninput=reset;form.elements.mode.onchange=reset;reset();
   for(const [selector,op] of [['[data-proxy-secret]','edit'],['[data-proxy-delete]','delete']])root.querySelector(selector).onclick=async()=>{
    try{await home.requestCapability(op,{accountId:'proxy:password'},'natives:secret');reset();}catch{status.textContent=t('fundHomeSettingsCredentialFailed');}
   };
   root.querySelector('[data-proxy-probe]').onclick=async e=>{
    const draft=value();signature=JSON.stringify(draft);probeToken=null;e.target.disabled=true;status.textContent=t('fundHomeLoading');
    try{const result=await api('POST','/api/settings/network/probe',{config:draft,expectedVersion:config.version});if(root.isConnected&&signature===JSON.stringify(value())){probeToken=result.probeToken;status.textContent=t('fundHomeNetworkVerified');}}
    catch{status.textContent=t('fundHomeNetworkFailed');}finally{e.target.disabled=form.elements.mode.value!=='https_proxy';}
   };
   form.onsubmit=async e=>{
    e.preventDefault();const draft=value();if(draft.mode==='https_proxy'&&(!probeToken||signature!==JSON.stringify(draft))){status.textContent=t('fundHomeNetworkNeedsProbe');return;}
    const button=form.querySelector('[type=submit]');button.disabled=true;
    try{await api('POST','/api/settings/network',{config:draft,expectedVersion:config.version,probeToken:probeToken||'',requestId});home.closePanel();await actions.loadFinanceSettings();}
    catch{status.textContent=t('fundHomeNetworkFailed');}finally{button.disabled=false;}
   };
  },true);
 }catch{home.status(t('fundHomeSettingsLoadFailed'));}
};
})();
