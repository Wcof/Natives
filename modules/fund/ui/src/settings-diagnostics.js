// Bounded diagnostics export: identifiers and status only, never free-text upstream errors.
(() => {
const {api,home,t}=Fund;
const esc=home.escape;
home.showFinancialDiagnostics=async()=>{
 try{
  const data=[];
  for(const path of ['/api/settings/health','/api/information/sources','/api/notify/channels','/api/notify/deliveries?page=1','/api/settings/network'])data.push(await api('GET',path));
  const [health,sources,channels,deliveries,network]=data;
  let model=null;try{model=await home.requestCapability('options');}catch{}
  const safeStatus=v=>['sent','failed','cancelled','skipped_silent','skipped_dedup'].includes(v)?v:'unknown';
  const safeError=v=>['network_error','upstream_business_error'].includes(v)?v:v?'unclassified_error':'';
  const output={formatVersion:1,generatedAt:new Date().toISOString(),module:'fund',
   transport:'authenticated-runtime-session',platformSignature:'not-verified-by-this-diagnostic',
   health:{schemaVersion:health.schemaVersion,dataRevision:health.dataRevision,databaseOk:health.databaseOk},
   network:{mode:network.mode,version:network.version},model:{defaultConfigured:Boolean(model?.defaultModel),optionsAvailable:Boolean(model)},
   sources:sources.sources.map(s=>({sourceId:s.sourceId,enabled:s.enabled,lastSuccessAt:s.lastSuccessAt,hasError:Boolean(s.lastErrorCode)})),
   channels:channels.channels.map(c=>({id:c.id,kind:c.kind,enabled:c.enabled,archived:c.archived,isDefault:c.isDefault,version:c.version,credentialsConfigured:Boolean(c.requiredSecretFields?.length)&&c.requiredSecretFields.every(k=>c.secrets?.[k]?.configured),lastTestOk:c.lastTestOk})),
   deliveries:deliveries.deliveries.map(d=>({id:d.id,channelId:d.channelId,status:safeStatus(d.status),errorCode:safeError(d.errorCode),createdAt:d.createdAt})),
   limits:{deliveries:20,personalData:false,secretData:false}};
  home.panel(()=>t('fundHomeSettingsDiagnostics'),()=>`<p>${esc(t('fundHomeSettingsDiagnosticsBoundary'))}</p><textarea readonly data-diagnostics aria-label="${esc(t('fundHomeSettingsDiagnostics'))}">${esc(JSON.stringify(output,null,2))}</textarea><button type="button" data-select-diagnostics>${esc(t('fundHomeTemplateSelect'))}</button><p>${esc(t('fundHomeTemplateCopyHint'))}</p>`,root=>{
   root.querySelector('[data-select-diagnostics]').onclick=()=>{const field=root.querySelector('[data-diagnostics]');field.focus();field.select();};
  },true);
 }catch{home.status(t('fundHomeSettingsLoadFailed'));}
};
})();
