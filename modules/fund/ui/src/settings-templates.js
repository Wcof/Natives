// Non-secret templates. Preview is read-only; commit is atomic and revision bound.
(() => {
const {api,home,t,actions}=Fund;
const esc=home.escape;
const keys={settings:'fundHomeSettingsDisplay',accounts:'fundHomeSettingsAccounts',strategies:'fundHomeSettingsStrategy',rules:'fundHomeSettingsRules',sources:'fundHomeSettingsSources',channels:'fundHomeSettingsChannels',watchlists:'fundHomeTemplateWatchlists'};
const modules=Object.keys(keys);
const choices=()=>`<fieldset><legend>${esc(t('fundHomeTemplateModules'))}</legend>${modules.map(key=>`<label><input type="checkbox" data-template-module="${key}" checked>${esc(t(keys[key]))}</label>`).join('')}</fieldset>`;
const selectModules=(template,node)=>{
 const copy=structuredClone(template);copy.modules=[...node.querySelectorAll('[data-template-module]:checked')].map(el=>el.dataset.templateModule);
 if(!copy.modules.length)throw Error('empty-selection');
 for(const key of modules)if(!copy.modules.includes(key))copy[key]=key==='settings'?null:[];
 return copy;
};
home.manageConfigTemplates=()=>{
 const root=home.panel(()=>t('fundHomeTemplateTitle'),()=>`<p>${esc(t('fundHomeTemplateBoundary'))}</p>${choices()}<button type="button" data-export>${esc(t('fundHomeTemplateExport'))}</button><label>${esc(t('fundHomeTemplateFile'))}<input type="file" accept=".json,application/json" data-file></label><textarea data-export-text readonly hidden aria-label="${esc(t('fundHomeTemplateExport'))}"></textarea><button type="button" data-select hidden>${esc(t('fundHomeTemplateSelect'))}</button><p data-template-status role="status"></p><div data-template-preview></div><button type="button" data-commit hidden>${esc(t('fundHomeTemplateCommit'))}</button>`,node=>{
  let generation=0,command=null,loaded=null,donor=false;
  const status=node.querySelector('[data-template-status]');
  const commit=node.querySelector('[data-commit]');
  node.querySelector('[data-export]').onclick=async e=>{
   e.target.disabled=true;
   try{const template=selectModules(await api('GET','/api/settings/export'),node);if(!node.isConnected)return;
    const output=node.querySelector('[data-export-text]');output.hidden=false;output.value=JSON.stringify(template,null,2);node.querySelector('[data-select]').hidden=false;
    status.textContent=t('fundHomeTemplateCopyHint');
   }catch{status.textContent=t('fundHomeSettingsLoadFailed');}finally{e.target.disabled=false;}
  };
  node.querySelector('[data-select]').onclick=()=>{const output=node.querySelector('[data-export-text]');output.focus();output.select();status.textContent=t('fundHomeTemplateCopyHint');};
  async function showPreview(){
   const gen=++generation;commit.hidden=true;command=null;
   try{const template=selectModules(loaded,node);const preview=await api('POST','/api/settings/import/preview',template);
    if(!node.isConnected||gen!==generation)return;
    command={template:preview.template,previewToken:preview.previewToken,expectedRevision:preview.expectedRevision,requestId:home.requestId()};
    node.querySelector('[data-template-preview]').innerHTML=`<h3>${esc(t('fundHomeTemplatePreview'))}</h3>${Object.entries(preview.counts).map(([key,count])=>`<p>${esc(t(keys[key]))}: ${Number(count)}</p>`).join('')}<p>${esc(t('fundHomeTemplateConflict'))}: ${preview.conflicts.length}</p><p>${esc(t('fundHomeTemplateCredentials'))}: ${preview.requiresCredentials}</p><p>${esc(t('fundHomeTemplateDisabled'))}</p>${donor?`<p>${esc(t('fundHomeTemplateDonorSkipped'))}</p>`:''}`;
    status.textContent='';commit.hidden=false;
   }catch{status.textContent=t('fundHomeTemplateInvalid');}
  }
  node.querySelectorAll('[data-template-module]').forEach(el=>el.onchange=()=>{node.querySelector('[data-export-text]').hidden=true;node.querySelector('[data-select]').hidden=true;if(loaded)showPreview();});
  node.querySelector('[data-file]').onchange=async e=>{
   const gen=++generation;commit.hidden=true;command=null;loaded=null;node.querySelector('[data-template-preview]').textContent='';
   const file=e.target.files?.[0];if(!file)return;
   if(file.size>200*1024){status.textContent=t('fundHomeTemplateTooLarge');return;}
   status.textContent=t('fundHomeLoading');
   try{
    const template=JSON.parse(await file.text());const preview=await api('POST','/api/settings/import/preview',template);
    if(!node.isConnected||gen!==generation)return;
    loaded=preview.template;donor=preview.donorMapped;await showPreview();
   }catch{status.textContent=t('fundHomeTemplateInvalid');}
  };
  commit.onclick=async()=>{
   if(!command||!await home.confirm(t('fundHomeTemplateConfirm')))return;
   commit.disabled=true;
   try{await api('POST','/api/settings/import/commit',command);home.closePanel();await actions.loadFinanceSettings();}
   catch{status.textContent=t('fundHomeTemplateStale');commit.disabled=false;}
  };
 },true);
 return root;
};
})();
