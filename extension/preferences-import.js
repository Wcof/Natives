import { previewLegacyPreferences, importLegacyPreferences } from './platform/storage.js';

export async function openPreferencesImport({t,returnFocus}) {
  if (document.getElementById('shared-preferences-import')) return;
  const dialog = document.createElement('dialog');
  dialog.id = 'shared-preferences-import';
  dialog.className = 'shared-preferences-dialog';
  dialog.setAttribute('aria-labelledby','shared-preferences-title');
  const title = document.createElement('h2');
  title.id = 'shared-preferences-title'; title.textContent=t('sharedPreferencesImport');
  const description=document.createElement('p'); description.textContent=t('sharedPreferencesImportDescription');
  const content=document.createElement('div'); content.className='shared-preferences-diff';
  const status=document.createElement('p'); status.setAttribute('role','status');
  const actions=document.createElement('div'); actions.className='actions';
  const cancel=document.createElement('button'); cancel.type='button'; cancel.textContent=t('cancel');
  const save=document.createElement('button'); save.type='button'; save.textContent=t('sharedPreferencesImportConfirm'); save.disabled=true;
  actions.append(cancel,save); dialog.append(title,description,content,status,actions);
  const container=returnFocus?.closest('dialog')||document.body;
  container.append(dialog); dialog.showModal(); cancel.focus();
  dialog.addEventListener('close',()=>{dialog.remove();returnFocus?.focus();},{once:true});
  cancel.onclick=()=>dialog.close();
  try {
    const {legacy,current}=await previewLegacyPreferences();
    if (!dialog.open) return;
    const entries={};
    for (const [key,value] of Object.entries(legacy)) {
      const row=document.createElement('p');
      const exists=Object.hasOwn(current.entries,key);
      row.textContent=`${key}: ${JSON.stringify(value)} → ${exists ? JSON.stringify(current.entries[key]) : t('sharedPreferencesWillImport')}`;
      content.append(row); if (!exists) entries[key]=value;
    }
    save.disabled=!Object.keys(entries).length;
    if (!Object.keys(legacy).length) status.textContent=t('sharedPreferencesNothingToImport');
    save.onclick=async()=>{
      save.disabled=true;
      try { await importLegacyPreferences(entries); dialog.close(); }
      catch { status.textContent=t('sharedPreferencesFailed'); save.disabled=false; }
    };
  } catch { status.textContent=t('sharedPreferencesFailed'); }
}
