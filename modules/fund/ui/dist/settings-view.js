// Fund settings surface. Every persistent value is read from its existing owner.
(() => {
const {api,home,actions,state,t}=Fund;
const esc=home.escape;
const view=document.getElementById('view-settings');
const body=document.getElementById('financial-settings-body');
const status=document.getElementById('financial-settings-status');
let generation=0,controller=null,model=null,sendTask=null,sendRequest=null;
let category='allocation';
const categories=[['allocation','fundHomeSettingsAllocation'],['accounts','fundHomeSettingsDisplay'],['strategy','fundHomeSettingsStrategy'],['ai','fundHomeSettingsAI'],['sources','fundHomeSettingsSources'],['notify','fundHomeSettingsChannels'],['system','fundHomeSettingsDiagnostics'],['transfer','fundHomeTemplateTitle']];
const categoryNav=document.getElementById('financial-settings-categories');
const categorySelect=document.getElementById('financial-settings-category');
function selectCategory(value){
 category=categories.some(([id])=>id===value)?value:'allocation';
 body.querySelectorAll('[data-settings-category]').forEach(section=>section.hidden=section.dataset.settingsCategory!==category);
 categoryNav.querySelectorAll('button').forEach(button=>{const chosen=button.dataset.category===category;button.setAttribute('aria-selected',String(chosen));button.tabIndex=chosen?0:-1;});
 categorySelect.value=category;
}
function renderCategories(){
 categoryNav.replaceChildren();categorySelect.replaceChildren();
 categories.forEach(([id,key])=>{
  const button=document.createElement('button');button.type='button';button.textContent=t(key);button.dataset.category=id;button.setAttribute('role','tab');button.onclick=()=>selectCategory(id);categoryNav.append(button);
  const option=document.createElement('option');option.value=id;option.textContent=t(key);categorySelect.append(option);
 });
 selectCategory(category);
}
categorySelect.addEventListener('change',()=>selectCategory(categorySelect.value));
categoryNav.addEventListener('keydown',e=>{
 if(!['ArrowDown','ArrowUp','Home','End'].includes(e.key))return;
 const buttons=[...categoryNav.querySelectorAll('button')],index=buttons.indexOf(document.activeElement);if(index<0)return;
 e.preventDefault();const next=e.key==='Home'?0:e.key==='End'?buttons.length-1:(index+(e.key==='ArrowDown'?1:buttons.length-1))%buttons.length;
 buttons[next].click();buttons[next].focus();
});
const settingsKeys={"AI": "fundHomeSettingsAI", "Model": "fundHomeSettingsModel", "ModelOwner": "fundHomeSettingsModelOwner", "Display": "fundHomeSettingsDisplay", "Color": "fundHomeSettingsColor", "Links": "fundHomeSettingsLinks", "auto": "fundHomeSettingsauto", "cn": "fundHomeSettingscn", "intl": "fundHomeSettingsintl", "eastmoney": "fundHomeSettingseastmoney", "xueqiu": "fundHomeSettingsxueqiu", "Allocation": "fundHomeSettingsAllocation", "Accounts": "fundHomeSettingsAccounts", "Strategy": "fundHomeSettingsStrategy", "Rules": "fundHomeSettingsRules", "Sources": "fundHomeSettingsSources", "Channels": "fundHomeSettingsChannels", "ChannelConsent": "fundHomeSettingsChannelConsent", "Add": "fundHomeSettingsAdd", "Deliveries": "fundHomeSettingsDeliveries", "Credentials": "fundHomeSettingsCredentials", "NotConfigured": "fundHomeSettingsNotConfigured", "Default": "fundHomeSettingsDefault", "Restore": "fundHomeSettingsRestore", "Edit": "fundHomeSettingsEdit", "Disable": "fundHomeSettingsDisable", "Enable": "fundHomeSettingsEnable", "Test": "fundHomeSettingsTest", "Archive": "fundHomeSettingsArchive", "EmptyChannels": "fundHomeSettingsEmptyChannels", "Policy": "fundHomeSettingsPolicy", "SilenceStart": "fundHomeSettingsSilenceStart", "SilenceEnd": "fundHomeSettingsSilenceEnd", "Retries": "fundHomeSettingsRetries", "Backoff": "fundHomeSettingsBackoff", "Dedup": "fundHomeSettingsDedup", "Timezone": "fundHomeSettingsTimezone", "SourceHealth": "fundHomeSettingsSourceHealth", "Unknown": "fundHomeSettingsUnknown", "Feedback": "fundHomeSettingsFeedback", "Sample": "fundHomeSettingsSample", "Useful": "fundHomeSettingsUseful", "Reports": "fundHomeSettingsReports", "Health": "fundHomeSettingsHealth", "DatabaseOK": "fundHomeSettingsDatabaseOK", "DatabaseFailed": "fundHomeSettingsDatabaseFailed", "Schema": "fundHomeSettingsSchema", "Revision": "fundHomeSettingsRevision", "LocalBoundary": "fundHomeSettingsLocalBoundary", "LoadFailed": "fundHomeSettingsLoadFailed", "WriteFailed": "fundHomeSettingsWriteFailed", "AddChannel": "fundHomeSettingsAddChannel", "EditChannel": "fundHomeSettingsEditChannel", "Name": "fundHomeSettingsName", "Kind": "fundHomeSettingsKind", "TrustedCredential": "fundHomeSettingsTrustedCredential", "Configure": "fundHomeSettingsConfigure", "RemoveCredential": "fundHomeSettingsRemoveCredential", "CredentialFailed": "fundHomeSettingsCredentialFailed", "TestConsent": "fundHomeSettingsTestConsent", "TestBody": "fundHomeSettingsTestBody", "TestSent": "fundHomeSettingsTestSent", "TestFailed": "fundHomeSettingsTestFailed"};
const label=key=>t(settingsKeys[key]);
const button=(action,key)=>`<button type="button" class="btn" data-settings-action="${action}">${esc(label(key))}</button>`;
const input=(name,key,value,type='text',extra='')=>home.input(name,label(key),value,type,extra);
const request=(op,payload={},type='natives:ai')=>home.requestCapability(op,payload,type);
function leave(){generation++;controller?.abort();controller=null;
 const target=sendTask?{taskId:sendTask}:sendRequest?{requestId:sendRequest}:null;sendTask=null;sendRequest=null;
 if(target)api("POST","/api/notify/cancel",target).catch(()=>{});
}
actions.leaveFinanceSettings=leave;
function applyDisplay(config){
 state.financialSettingsVersion=config.version;
 state.linkPlatform=config.linkPlatform;
 if(config.benchmarkSymbol)state.benchmarkSymbol=config.benchmarkSymbol;
 if(config.quoteRefreshSeconds)state.quoteRefreshSeconds=config.quoteRefreshSeconds;
 state.financialColorMode=config.colorMode;
 state.colorMode=config.colorMode==='auto'?(state.language==='en'?'intl':'cn'):config.colorMode;
 document.documentElement.dataset.colorMode=state.colorMode;
 if(state.colorMode==='intl')document.documentElement.dataset.colorMode='intl';
 actions.redrawChart?.();actions.reloadStockDetailTheme?.();Fund.trends?.reloadTheme?.();
 document.dispatchEvent(new Event('fund:financial-display-changed'));
}
actions.applyFinancialDisplay=applyDisplay;
actions.loadFinancialDisplay=async()=>{try{applyDisplay(await api('GET','/api/settings'));}catch{home.status(label('LoadFailed'));}};
actions.setFinancialColorMode=async colorMode=>{
 const settings=await api('GET','/api/settings');
 const saved=await api('POST','/api/settings',{settings:{colorMode,linkPlatform:settings.linkPlatform,sourcePriority:settings.sourcePriority,benchmarkSymbol:settings.benchmarkSymbol,quoteRefreshSeconds:settings.quoteRefreshSeconds,aiMaxTokens:settings.aiMaxTokens},expectedVersion:settings.version,requestId:home.requestId()});
 applyDisplay(saved);
};
function render(){
 if(!model)return;
 const draft=[...body.querySelectorAll('input,select,textarea')].map(el=>({form:el.form?.dataset.financialDisplay!==undefined?'display':el.form?.dataset.aiSettings!==undefined?'ai':'policy',name:el.name,value:el.value}));
 const dirty=[...body.querySelectorAll('form')].map(form=>form.dataset.dirty==='true');
 const focused=body.contains(document.activeElement)?document.activeElement?.name:null;
 const {settings,channels,policy,sources,stats,options,health,overview}=model;
 const privateMode=home.displayMode()!=='amount',identityHidden=home.displayMode()==='hidden';
 const cash=overview?.wealth?.cash,groups=overview?.groupAllocation?.groups||[],goal=overview?.goals?.goal;
 const amount=value=>privateMode?'****':value==null?t('fundDetailMissing'):home.money(value);
 const ratio=value=>identityHidden?'****':value==null?t('fundDetailMissing'):(Number(value)/10).toFixed(1)+'%';
 const select=(name,key,values,value)=>home.select(name,label(key),values.map(v=>[v,label(v)]),value);
 const channelRows=channels.channels.map(c=>`<article data-channel="${c.id}"><h4>${esc(c.name)} · ${esc(c.kind)}</h4><p>${esc(t(c.archived?'fundHomeArchived':c.enabled?'fundHomeEnabled':'fundHomeDisabled'))}${c.isDefault?' · '+esc(label('Default')):''}</p><p>${esc(label('Credentials'))}: ${Object.entries(c.secrets||{}).filter(([,v])=>v.configured).map(([k])=>esc(k)).join(', ')||esc(label('NotConfigured'))}</p>${c.archived?button('restore-'+c.id,'Restore'):button('edit-'+c.id,'Edit')+button('secrets-'+c.id,'Credentials')+button('toggle-'+c.id,c.enabled?'Disable':'Enable')+button('default-'+c.id,'Default')+button('test-'+c.id,'Test')+button('archive-'+c.id,'Archive')}</article>`).join('');
 body.innerHTML=`<section data-settings-category="ai"><h3>${esc(label('AI'))}</h3><p>${esc(options?.defaultModel?(options.defaultModel.alias||[options.defaultModel.providerId,options.defaultModel.modelId].filter(Boolean).join('/')):label('NotConfigured'))}</p>${button('model','Model')}<form data-ai-settings>${home.input('aiMaxTokens',t('fundHomeAiTokenBudget'),settings.aiMaxTokens??4096,'number','min="512" max="8192" step="1" required')}<p class="investment-card-tip">${esc(t('fundHomeAiTokenBudgetHint'))}</p><button class="btn" type="submit">${esc(t('fundHomeSave'))}</button></form><p>${esc(label('ModelOwner'))}</p></section>
 <section data-settings-category="accounts"><h3>${esc(label('Display'))}</h3><form id="financial-display" data-financial-display>${select('colorMode','Color',['auto','cn','intl'],settings.colorMode)}${select('linkPlatform','Links',['eastmoney','xueqiu'],settings.linkPlatform)}${home.select('sourcePriority',t('fundHomeSettingsSourcePriority'),[['em_stock_ann',t('fundHomeStockAnnouncements')],['em_fund_ann',t('fundHomeFundAnnouncements')],['em_stock_news',t('fundHomeStockNews')]],settings.sourcePriority)}${home.select('benchmarkSymbol',t('fundHomeSettingsBenchmark'),[['sh000300',t('fundHomeIndexCsi300')],['sh000016',t('fundHomeIndexSse50')],['sh000905',t('fundHomeIndexCsi500')],['sh000001',t('fundHomeIndexShanghai')],['sz399006',t('fundHomeIndexChiNext')]],settings.benchmarkSymbol)}${home.select('quoteRefreshSeconds',t('fundHomeSettingsRefresh'),[3,10,30,60].map(v=>[v,String(v)]),settings.quoteRefreshSeconds)}<button class="btn" type="submit">${esc(t('fundHomeSave'))}</button></form></section>
 <section data-settings-category="allocation" class="settings-plan-overview"><div class="settings-overview-grid"><section class="card"><h3>${esc(t('fundSettingsConstraints'))}</h3><dl class="metric-list"><div><dt>${esc(t('fundHomeCash'))}</dt><dd>${esc(amount(cash?.cashMinor))}</dd></div><div><dt>${esc(t('fundSettingsReserveLabel'))}</dt><dd>${esc(amount(cash?.reservedMinor))}</dd></div><div><dt>${esc(t('fundSettingsAvailableLabel'))}</dt><dd>${esc(amount(cash?.availableMinor))}</dd></div></dl><p class="investment-card-tip">${esc(t('fundSettingsConstraintsHint'))}</p><div class="row"><button type="button" data-settings-action="cash">${esc(t('fundHomeCashAndFlowReconciliation'))}</button>${button('rules','Rules')}${button('strategy','Strategy')}</div></section><div class="stack"><section class="card"><div class="between"><h3>${esc(t('fundHomeGroupPlanTitle'))}</h3>${button('allocation','Edit')}</div><div class="combination-columns"><span>${esc(t('fundHomeGroup'))}</span><span>${esc(t('fundHomeGroupTarget'))}</span><span>${esc(t('fundHomeGroupActual'))}</span></div>${groups.map(g=>`<div class="combination-row"><div class="combination-values"><strong>${identityHidden?'****':esc(g.name)}</strong><span>${esc(ratio(g.ratioTenths))}</span><span>${esc(ratio(g.currentTenths))}</span></div><p class="allocation-tip">${identityHidden?'':esc(g.description||t('fundHomeGroupDescriptionEmptyHint'))}</p></div>`).join('')||`<p>${esc(t('fundHomeGroupDescriptionEmptyHint'))}</p>`}</section><section class="card"><div class="between"><h3>${esc(t('fundHomeGoalSettings'))}</h3><button type="button" data-settings-action="goal">${esc(t('fundHomeEdit'))}</button></div><small>${esc(t('fundHomeTargetAmountCny'))}</small><div class="holding-amount">${esc(amount(goal?.targetMinor))}</div><p>${esc(t('fundHomeGoalCalculatedOnRegisteredAssets'))}</p></section><section class="card"><h3>${esc(t('fundSettingsOther'))}</h3><div class="row">${button('accounts','Accounts')}${button('sources','Sources')}${button('model','Model')}</div></section></div></div></section><section data-settings-category="strategy"><h3>${esc(label('Strategy'))}</h3>${button('strategy','Strategy')}${button('rules','Rules')}</section>
 <section data-settings-category="notify"><h3>${esc(t('fundHomeNotificationPermission'))}</h3><p>${esc(t(model.permission==='granted'?'fundHomeNotificationGranted':model.permission==='denied'?'fundHomeNotificationDenied':'fundHomeSettingsUnknown'))}</p><p>${esc(t('fundHomeNotificationScope'))}</p>${button('rules','Rules')}</section>
 <section data-settings-category="notify"><h3>${esc(label('Channels'))}</h3><p>${esc(label('ChannelConsent'))}</p>${button('add-channel','Add')}${button('deliveries','Deliveries')}${channelRows||`<p>${esc(label('EmptyChannels'))}</p>`}</section>
 <section data-settings-category="notify"><h3>${esc(label('Policy'))}</h3><form data-notify-policy>${input('silenceStart','SilenceStart',policy.silenceStart,'time')}${input('silenceEnd','SilenceEnd',policy.silenceEnd,'time')}${input('retryMax','Retries',policy.retryMax,'number','min="0" max="3" required')}${input('backoffBase','Backoff',policy.backoffBase,'number','min="1" max="30" required')}${input('dedupMinutes','Dedup',policy.dedupMinutes,'number','min="1" max="1440" required')}<p>${esc(label('Timezone'))}</p><button class="btn" type="submit">${esc(t('fundHomeSave'))}</button></form></section>
 <section data-settings-category="sources"><h3>${esc(label('SourceHealth'))}</h3>${sources.sources.map(s=>`<p>${esc(s.sourceId)} · ${esc(t(s.enabled?'fundHomeEnabled':'fundHomeDisabled'))} · ${esc(s.lastSuccessAt||label('Unknown'))} · ${esc(s.lastErrorCode||'')}</p>`).join('')}${button('sources','Sources')}<button type="button" data-settings-action="cache">${esc(t('fundHomeSettingsCacheClear'))}</button></section>
 <section data-settings-category="transfer"><h3>${esc(label('Feedback'))}</h3><p>${esc(label('Sample'))}: ${stats.feedback?.total??0} · ${esc(label('Useful'))}: ${stats.feedback?.usefulRatePpm==null?esc(label('Unknown')):(stats.feedback.usefulRatePpm/10000).toFixed(2)+'%'}</p>${button('reports','Reports')}<button type="button" data-settings-action="report-feedback">${esc(t('fundHomeReportFeedback'))}</button></section>
 <section data-settings-category="transfer"><h3>${esc(t('fundHomeTemplateTitle'))}</h3><button type="button" data-settings-action="templates">${esc(t('fundHomeTemplateTitle'))}</button></section>
 <section data-settings-category="system"><h3>${esc(t('fundHomeNetworkTitle'))}</h3><button type="button" data-settings-action="network">${esc(t('fundHomeNetworkTitle'))}</button></section>
 <section data-settings-category="system"><h3>${esc(label('Health'))}</h3><p>${esc(label(health.databaseOk?'DatabaseOK':'DatabaseFailed'))} · ${esc(label('Schema'))}: ${health.schemaVersion} · ${esc(label('Revision'))}: ${esc(health.dataRevision)}</p><p>${esc(label('LocalBoundary'))}</p><button type="button" data-settings-action="diagnostics">${esc(t("fundHomeSettingsDiagnostics"))}</button></section>`;
 const display=body.querySelector('[data-financial-display]');
 display.onsubmit=async e=>{e.preventDefault();const draft=Object.fromEntries(new FormData(display));draft.quoteRefreshSeconds=Number(draft.quoteRefreshSeconds);draft.aiMaxTokens=model.settings.aiMaxTokens;await mutate(display,async()=>{model.settings=await api('POST','/api/settings',{settings:draft,expectedVersion:model.settings.version,requestId:home.requestId()});applyDisplay(model.settings);});};
 const aiForm=body.querySelector('[data-ai-settings]');
 aiForm.onsubmit=async e=>{e.preventDefault();const {version,...settings}=model.settings;settings.aiMaxTokens=Number(aiForm.elements.aiMaxTokens.value);await mutate(aiForm,async()=>{model.settings=await api('POST','/api/settings',{settings,expectedVersion:version,requestId:home.requestId()});});};
 const policyForm=body.querySelector('[data-notify-policy]');
 policyForm.onsubmit=async e=>{e.preventDefault();const draft=Object.fromEntries(new FormData(policyForm));for(const k of ['retryMax','backoffBase','dedupMinutes'])draft[k]=Number(draft[k]);await mutate(policyForm,async()=>{const saved=await api('POST','/api/notify/settings',{...draft,expectedVersion:model.policy.version,requestId:home.requestId()});model.policy={...draft,version:saved.version};});};
 for(const saved of draft){const form=saved.form==='display'?display:saved.form==='ai'?aiForm:policyForm;if(form.elements[saved.name])form.elements[saved.name].value=saved.value;}
 for(const [index,form] of [aiForm,display,policyForm].entries()){
  const feedback=document.createElement('p');feedback.dataset.saveStatus='';feedback.setAttribute('role','status');form.append(feedback);
  form.dataset.dirty=String(Boolean(dirty[index]));
  if(dirty[index])feedback.textContent=t('fundSettingsUnsaved');
   for(const control of form.elements)control.addEventListener('input',()=>{form.dataset.dirty='true';feedback.textContent=t('fundSettingsUnsaved');});
 }
 renderCategories();
 if(focused)body.querySelector(`[name="${CSS.escape(focused)}"]`)?.focus({preventScroll:true});
}
async function mutate(form,work){
 const button=form.querySelector('button[type=submit]');if(button.disabled)return;
 button.disabled=true;const focus=document.activeElement;form.inert=true;
 const feedback=form.querySelector('[data-save-status]');
 try{await work();form.dataset.dirty='false';feedback.textContent=t('fundHomeSaved');}
 catch(error){feedback.textContent=error.status===409?t('fundHomeDataConflict'):label('WriteFailed');}
 finally{button.disabled=false;form.inert=false;if(focus?.isConnected&&form.contains(focus))focus.focus({preventScroll:true});}
}
async function load(){
 leave();const gen=generation;controller=new AbortController();status.textContent=t('fundHomeLoading');view.setAttribute('aria-busy','true');
 try{
  const signal=controller.signal;
  const read=url=>api('GET',url,null,{signal});
  // Respect the Runtime's bounded HTTP worker pool, including visible tickers.
  // A replaced generation must stop before issuing its next request.
  const data=[];
  for(const path of ['/api/settings','/api/notify/channels','/api/notify/settings','/api/information/sources','/api/opportunities/stats','/api/settings/health','/api/home/snapshot']){
   if(gen!==generation||signal.aborted)return;
   data.push(await read(path));
  }
  const [settings,channels,policy,sources,stats,health,overview]=data;
  let options=null;try{options=await request('options');}catch{options=null;}
  if(gen!==generation||view.hidden)return;
  let permission='unknown';try{permission=(await request('status',{},'natives:notification'))?.permission||'unknown';}catch{}
  if(gen!==generation||view.hidden)return;
  model={settings,channels,policy,sources,stats,health,options,permission,overview};applyDisplay(settings);status.textContent='';render();
 }catch(e){if(gen===generation&&e.name!=='AbortError')status.textContent=label('LoadFailed');}
 finally{if(gen===generation){view.setAttribute('aria-busy','false');controller=null;}}
}
actions.loadFinanceSettings=load;
for(const event of ['fund:privacy-changed','fund:home-snapshot-updated'])document.addEventListener(event,()=>{if(model){if(event==='fund:home-snapshot-updated')model.overview=home.snapshot();render();}});
async function channelDialog(channel={}){
 const edit=Boolean(channel.id);let kind=channel.kind||'telegram';
 const kinds=['telegram','bark','dingtalk','wecom','feishu','serverchan','pushplus','discord','pushover'];
 const fields={telegram:['chat_id'],bark:['server_url'],dingtalk:['phones','keyword'],pushplus:['topic'],discord:['webhook_id']};
 const draft={name:channel.name||'',config:{...channel.config}};
 home.panel(()=>label(edit?'EditChannel':'AddChannel'),()=>`<form>${home.input('name',label('Name'),draft.name,'text','required maxlength="64"')}${home.select('kind',label('Kind'),kinds.map(k=>[k,k]),kind)}<div data-fields>${(fields[kind]||[]).map(k=>home.input(k,k,draft.config[k]||'','text','maxlength="512"')).join('')}</div><button type="submit" class="btn">${esc(t('fundHomeSave'))}</button></form>`,root=>{
  const form=root.querySelector('form');form.elements.kind.disabled=edit;
  form.elements.kind.onchange=()=>{kind=form.elements.kind.value;form.querySelector('[data-fields]').innerHTML=(fields[kind]||[]).map(k=>home.input(k,k,'','text','maxlength="512"')).join('');};
  home.submit(root,async v=>{const config=Object.fromEntries((fields[kind]||[]).map(k=>[k,v[k]||'']));await api('POST','/api/notify/channels/save',{kind,name:v.name,config,requestId:home.requestId(),...(edit?{id:channel.id,expectedVersion:channel.version}:{})});home.closePanel();await load();});
 },true);
}
async function credentials(channel){
 const fields={telegram:['bot_token'],bark:['device_key'],dingtalk:['token','secret'],wecom:['webhook_key'],feishu:['webhook_token'],serverchan:['sendkey'],pushplus:['token'],discord:['webhook_token'],pushover:['user_key','app_token']}[channel.kind];
 home.panel(()=>label('Credentials'),()=>`<p>${esc(label('TrustedCredential'))}</p>${channel.kind==='dingtalk'?`<p>${esc(t('fundHomeNotifyDingSigningOptional'))}</p>`:''}${fields.map(k=>`<p>${esc(k)} <button type="button" data-credential="${k}">${esc(label('Configure'))}</button> <button type="button" data-clear="${k}">${esc(label('RemoveCredential'))}</button></p>`).join('')}`,root=>{
  root.querySelectorAll('[data-credential],[data-clear]').forEach(b=>b.onclick=async()=>{
   b.disabled=true;
   try{const field=b.dataset.credential||b.dataset.clear;const result=await request(b.dataset.clear?'delete':'edit',{accountId:`channel:${channel.id}:${field}`},'natives:secret');if(result.cancelled)return;
    const fresh=await api('GET','/api/notify/channels');const version=fresh.channels.find(c=>c.id===channel.id).version;
    await api('POST','/api/notify/channels/secret',{id:channel.id,field,expectedVersion:version});await load();
   }catch{home.failure(Error(label('CredentialFailed')));}finally{b.disabled=false;}
  });
 },true);
}
body.addEventListener('click',async e=>{
 const b=e.target.closest('[data-settings-action]');if(!b||!model)return;
 const action=b.dataset.settingsAction;status.textContent='';
 const simple={cash:()=>home.manageCash(),goal:()=>home.manageGoals(),network:()=>home.manageNetworkSettings(),'report-feedback':()=>home.showReportFeedbackSummary(),diagnostics:()=>home.showFinancialDiagnostics(),templates:()=>home.manageConfigTemplates(),allocation:()=>home.manageGroupPlan(),accounts:()=>actions.manageAccounts(),strategy:()=>home.manageStrategies(),rules:()=>home.manageRules(),sources:()=>home.manageSources(),reports:()=>home.manageReports()};
 try{
  if(simple[action]){await simple[action]();return;}
  if(action==='cache'){const current=await api('GET','/api/information/cache');if(!await home.confirm(t('fundHomeSettingsCacheConfirm')+' '+current.items))return;await api('POST','/api/information/cache',{expectedRevision:current.dataRevision,requestId:home.requestId()});status.textContent=t('fundHomeSaved');return;}
  if(action==='model'){
   // 跨 iframe 对话框关闭后，Shell 只能聚焦 iframe；内部触发点由模块恢复。
   const restore=()=>{if(b.isConnected&&!view.hidden)b.focus({preventScroll:true});};
   window.addEventListener('focus',restore,{once:true,signal:controller.signal});
   try{await request('open',{},'natives:model-settings');}
   catch(error){window.removeEventListener('focus',restore);throw error;}
   return;
  }
  if(action==='add-channel'){await channelDialog();return;}
  if(action==='deliveries'){
   let page=1;
   const show=async()=>{const result=await api('GET',`/api/notify/deliveries?page=${page}`);home.panel(()=>label('Deliveries'),()=>`<div>${result.deliveries.map(d=>`<article><h4>${esc(d.channelName)}</h4><p>${esc(t(({sent:"fundHomeSettingsTestSent",failed:"fundHomeSettingsTestFailed",cancelled:"fundHomeCancelled",skipped_silent:"fundHomeSettingsSilenceStart",skipped_dedup:"fundHomeSettingsDedup"})[d.status]||"fundHomeSettingsUnknown"))} · ${esc(new Date(Number(d.createdAt)*1000).toLocaleString(state.language==='en'?'en-US':'zh-CN'))}</p><p>${esc(d.errorCode)}</p></article>`).join('')||esc(label('EmptyChannels'))}</div><button data-previous ${page===1?'disabled':''}>${esc(t('fundHomePrevious'))}</button><button data-next ${result.deliveries.length<20?'disabled':''}>${esc(t('fundHomeNext'))}</button>`,root=>{root.querySelector('[data-previous]').onclick=()=>{page--;show().catch(()=>status.textContent=label('LoadFailed'));};root.querySelector('[data-next]').onclick=()=>{page++;show().catch(()=>status.textContent=label('LoadFailed'));};},true);};await show();return;
  }
  const [op,id]=action.split('-');const channel=model.channels.channels.find(c=>c.id===Number(id));if(!channel)return;
  if(op==='edit'){await channelDialog(channel);return;}
  if(op==='secrets'){await credentials(channel);return;}
  if(op==='test'){
   if(!await home.confirm(label('TestConsent')+' '+channel.name))return;
   const gen=generation;b.disabled=true;
   sendRequest=home.requestId();
   try{const started=await api('POST','/api/notify/channels/test',{id:channel.id,title:'Natives',body:label('TestBody'),bypassSilence:false,requestId:sendRequest});
    if(gen!==generation){await api('POST','/api/notify/cancel',{taskId:started.taskId});return;}
    sendTask=started.taskId;
    while(gen===generation&&sendTask){
     const current=await api('GET',`/api/notify/status?taskId=${sendTask}`,null,{signal:controller.signal});
     if(current.status!=='running'){sendTask=null;sendRequest=null;status.textContent=label(current.result?.status==='sent'?'TestSent':'TestFailed');break;}
     await new Promise(resolve=>{const timer=setTimeout(finish,250);const signal=controller.signal;function finish(){clearTimeout(timer);signal.removeEventListener('abort',finish);resolve();}signal.addEventListener('abort',finish,{once:true});});
    }
   }finally{b.disabled=false;}return;
  }
  const changes={toggle:{enabled:!channel.enabled},default:{isDefault:true},archive:{archived:true},restore:{restore:true}}[op];
  if(changes){await api('POST','/api/notify/channels/state',{id:channel.id,expectedVersion:channel.version,requestId:home.requestId(),...changes});await load();}
 }catch{status.textContent=label('WriteFailed');}
});
document.getElementById('financial-settings-reload').onclick=load;
document.addEventListener('fund:language-changed',()=>{if(state.financialColorMode==='auto')applyDisplay({colorMode:'auto',linkPlatform:state.linkPlatform});if(model&&!view.hidden)render();});
document.addEventListener('visibilitychange',()=>{if(document.hidden)leave();});
window.addEventListener('pagehide',leave);
})();
