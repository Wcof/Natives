// Feedback measures usefulness, independently from quoted returns.
(() => {
const {home,api,t}=Fund;
const {escape:esc,panel,check,input,failure}=home;
const taskKeys={diagnosis:'fundHomeTaskDiagnosis',conditions:'fundHomeTaskConditions',news_impact:'fundHomeTaskNewsImpact',briefing:'fundHomeTaskBriefing'};
home.reportTaskLabel=task=>t(taskKeys[task]||'fundHomeSettingsUnknown');
home.bindDecisionReports=root=>{
 const requests=new Set();root.addEventListener('close',()=>requests.forEach(controller=>controller.abort()),{once:true});
 root.querySelectorAll('[data-decision-report]').forEach(details=>{
  let cached=null,controller=null;
  details.addEventListener('toggle',async()=>{
   if(!details.open){controller?.abort();return;}
   const body=details.querySelector('[data-saved-report]');
   let owned=null;
   body.textContent=t('fundDetailLoading');
   try{
    if(!cached){owned=controller=new AbortController();requests.add(owned);
     const data=await api('GET',`/api/analysis/reports?id=${details.dataset.decisionReport}&includeDeleted=1`,null,{signal:owned.signal});
     if(owned.signal.aborted||!root.isConnected||!details.open||home.privacy())return;
     cached=data.reports[0];if(!cached)throw Error(t('fundHomeSavedReportMissing'));
    }
    const evidence=cached.evidence?.evidence||[],facts=evidence.find(piece=>piece.kind==='local-facts');
    body.innerHTML=`<p>${esc(home.reportTaskLabel(cached.task))} · ${esc(cached.modelName)} · ${esc(new Date(Number(cached.savedAt)*1000).toLocaleString(Fund.state.language))}${cached.deleted?' · '+esc(t('fundHomeArchived')):''}</p><p>${esc(t('fundHomeSavedReportCaptured'))}</p><small>${esc(cached.snapshotId)} · ${esc(cached.dataRevision)}</small>${home.renderDecisionResult(cached)}${facts?`<p>${esc(t('fundHomeLocalFacts'))} · ${esc(t(facts.quality==='ready'?'fundHomeOverviewComplete':'fundHomeIncompleteData'))} · ${esc(facts.asOf||t('fundHomeSourceTimestampUnknown'))}</p>`:''}${evidence.filter(piece=>piece.kind==='information').map(piece=>`<article><b>${esc(piece.title)}</b><p>${esc(piece.sourceId)} · ${esc(piece.publishedAt||t('fundHomeSourceTimestampUnknown'))}</p><p>${esc(piece.excerpt||piece.summary||t('fundHomeLinkOnlyFullTextNotRead'))}</p><small>${esc(t('fundHomeAiExcerptScope'))}</small></article>`).join('')}`;
   }catch(error){if(error.name!=='AbortError'&&!owned?.signal.aborted&&root.isConnected&&details.open)body.textContent=error.message;}
   finally{if(owned)requests.delete(owned);}
  });
 });
};
home.reportFeedbackMarkup=r=>`<p>${esc(t('fundHomeReportFeedback'))}: ${esc(t(r.feedback==null?'fundHomeReportUnrated':r.feedback.useful?'fundHomeReportUseful':'fundHomeReportNotUseful'))}</p>${r.deleted?'':`<button data-feedback-report="${r.id}">${esc(t('fundHomeReportFeedback'))}</button>`}<button data-feedback-history="${r.id}">${esc(t('fundHomeReportFeedbackHistory'))}</button>`;
home.bindReportFeedback=(root,reports)=>{
 root.querySelectorAll('[data-feedback-report]').forEach(b=>b.onclick=()=>{
  const r=reports.find(r=>r.id===Number(b.dataset.feedbackReport));
  panel(()=>t('fundHomeReportFeedback'),()=>`<form>${check('useful',t('fundHomeReportUseful'),r.feedback?.useful??true)}${input('reason',t('fundHomeReportFeedbackReason'),r.feedback?.reason||'','text','maxlength="200"')}<button type="submit">${esc(t('fundHomeSave'))}</button><p role="status" data-error></p></form>`,dialog=>{
   const form=dialog.querySelector('form');let requestId=home.requestId(),previous=null;
   form.onsubmit=async e=>{e.preventDefault();const submit=form.querySelector('[type=submit]');submit.disabled=true;
    const payload={id:r.id,expectedVersion:r.feedback?.version||0,useful:form.elements.useful.checked,reason:form.elements.reason.value};
    if(previous&&previous!==JSON.stringify(payload))requestId=home.requestId();previous=JSON.stringify(payload);
    try{await api('POST','/api/analysis/reports/feedback',{...payload,requestId});home.manageReports();}
    catch(e){dialog.querySelector('[data-error]').textContent=e.message;}
    finally{submit.disabled=false;}
   };
  });
 });
 root.querySelectorAll('[data-feedback-history]').forEach(b=>b.onclick=()=>history(Number(b.dataset.feedbackHistory),1).catch(failure));
};
async function history(id,page){
 const data=await api('GET',`/api/analysis/reports/feedback/history?id=${id}&page=${page}`);
 panel(()=>t('fundHomeReportFeedbackHistory'),()=>`${data.items.map(r=>`<article><b>${r.version} · ${esc(t(r.useful?'fundHomeReportUseful':'fundHomeReportNotUseful'))}</b><p>${esc(r.reason)}</p></article>`).join('')||esc(t('fundHomeReportUnrated'))}<button data-prev ${page===1?'disabled':''}>${esc(t('fundHomePrevious'))}</button><button data-next ${page*20>=data.total?'disabled':''}>${esc(t('fundHomeNext'))}</button>`,root=>{root.querySelector('[data-prev]').onclick=()=>history(id,page-1).catch(failure);root.querySelector('[data-next]').onclick=()=>history(id,page+1).catch(failure);});
}
home.showReportFeedbackSummary=async()=>{
 try{const stats=await api('GET','/api/analysis/reports/stats');
  const row=(r,label)=>`<article><h4>${esc(label)}</h4><p>${esc(t('fundHomeReportCount'))}: ${r.reports} · ${esc(t('fundHomeSettingsSample'))}: ${r.feedback} · ${esc(t('fundHomeReportUnrated'))}: ${r.unrated}</p><p>${esc(t('fundHomeSettingsUseful'))}: ${r.usefulRatePpm==null?esc(t('fundHomeSettingsUnknown')):(r.usefulRatePpm/10000).toFixed(2)+'%'}</p></article>`;
  panel(()=>t('fundHomeReportFeedback'),()=>`<p>${esc(t('fundHomeReportFeedbackMetric'))}</p>${row(stats.total,t('fundHomeAll'))}${stats.byTask.map(r=>row(r,t(taskKeys[r.task]||'fundHomeSettingsUnknown'))).join('')}`);
 }catch(e){failure(e);}
};
})();
