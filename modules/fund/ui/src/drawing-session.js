(() => {
// Per-chart persistence owner; renderers keep their own geometry and draft gestures.
const { api, t } = Fund;
const normalize = raw => {
  const s = String(raw || '').trim().toLowerCase();
  if (/^(sh|sz|bj|us)/.test(s)) return s;
  if (/^\d{6}$/.test(s)) {
    if (s === '000001' || /^(60|68|51|56|58)/.test(s)) return 'sh' + s;
    if (/^(00|30|15|16|39)/.test(s)) return 'sz' + s;
    if (/^(8|4|92)/.test(s)) return 'bj' + s;
  }
  return s;
};
const canonical = value => JSON.stringify(value, (_, v) => v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.keys(v).sort().map(k => [k, v[k]])) : v);
Fund.createDrawingSession = ({ scope, replace, statusRoot }) => {
  let confirmed = [], legacy = [], failed = null, busy = false, reading = null, generation = 0, loadedKey = '';
  const current = () => { const s = scope(); return { ...s, symbol: normalize(s.symbol) }; };
  const key = s => canonical(s);
  const rows = () => structuredClone(confirmed);
  let status;
  function message(text) {
    const root = statusRoot(); if (!root) return;
    if (!status || !status.isConnected) { status = document.createElement('div'); status.className = 'drawing-status'; status.setAttribute('role', 'status'); root.append(status); }
    status.replaceChildren();
    const label = document.createElement('span'); label.textContent = text; status.append(label);
    if (failed) {
      const retry = document.createElement('button'); retry.type = 'button'; retry.disabled = busy;
      retry.textContent = t(failed.conflict ? 'fundChartDrawingReview' : 'fundChartDrawingRetry');
      retry.hidden = key(current()) !== failed.key;
      retry.onclick = async () => {
        if (failed.conflict) {
          const job = failed;
          await load();
          if (job !== failed || key(current()) !== job.key) return;
          const actual = confirmed.find(d => d.id === job.draft.id);
          // Conflict is never silently rebased: user sees the actual current and draft values.
          if (!await Fund.home.confirm(t('fundChartDrawingConflictReview') + '\n' + t('fundChartDrawingCurrent') + ': ' + canonical(actual?.points ?? []) + '\n' + t('fundChartDrawingDraft') + ': ' + canonical(job.draft.points))) return;
          if (job !== failed || key(current()) !== job.key) return;
          if (!actual) { message(t('fundChartDrawingMissing')); return; }
          job.draft.digest = actual.digest;
        }
        const job = failed;
        if (job) { failed = null; job.remove ? remove(job.draft.id) : save(job.draft); }
      };
      status.append(retry);
      const discard = document.createElement('button'); discard.type = 'button'; discard.disabled = busy; discard.textContent = t('fundChartDrawingDiscard');
      discard.onclick = () => { failed = null; replace(rows()); message(''); };
      status.append(discard);
    }
    if (legacy.length) {
      const details = document.createElement('details'), summary = document.createElement('summary');
      summary.textContent = t('fundChartDrawingLegacy', { count: legacy.length }); details.append(summary);
      for (const d of legacy.slice(0, 100)) { const p = document.createElement('p'); p.textContent = [d.options?.name, d.toolType, d.id].filter(Boolean).join(' · '); details.append(p); }
      status.append(details);
    }
    status.hidden = !text && !failed && !legacy.length;
  }
  async function load() {
    reading?.abort(); reading = new AbortController(); const signal = reading.signal, seq = ++generation, s = current(), identity = key(s);
    if (identity !== loadedKey) { confirmed = []; legacy = []; loadedKey = identity; replace([]); }
    if (!s.symbol) return;
    try {
      const res = await api('GET', '/api/drawings?symbol=' + encodeURIComponent(s.symbol) + '&period=' + encodeURIComponent(s.period), null, { signal });
      if (seq !== generation || key(current()) !== identity) return;
      if (!Array.isArray(res.drawings)) throw new Error('DRAWINGS_RESPONSE');
      const belongs = d => d.options?.coordinateVersion === 2 && canonical(d.options.viewContext) === canonical(s.viewContext) && d.points.every(p => typeof p.date === 'string' && Number.isFinite(p.price));
      confirmed = res.drawings.filter(belongs); legacy = res.drawings.filter(d => !d.options?.viewContext || d.options.coordinateVersion !== 2);
      replace(rows()); message(failed ? t('fundChartDrawingFailed') : '');
    } catch (e) { if (!signal.aborted && seq === generation) message(t('fundChartDrawingLoadFailed')); }
  }
  async function save(draft) {
    if (busy || failed) {
      if (!failed) failed = { key: key(current()), draft: structuredClone(draft), conflict: false };
      replace(rows()); message(t('fundChartDrawingFailed')); return false;
    }
    const s = current(), identity = key(s), d = structuredClone(draft);
    d.symbol = normalize(d.symbol);
    d.options = { ...d.options, coordinateVersion: 2, viewContext: s.viewContext };
    const body = { id: d.id, symbol: d.symbol, period: d.period, toolType: d.toolType, points: d.points, options: d.options, ...(d.digest ? { expectedDigest: d.digest } : {}) };
    busy = true; message(t('fundChartDrawingSaving'));
    try {
      let result;
      try { result = await api('POST', '/api/drawings', body); }
      catch (e) {
        if (!e.status) {
          const check = await api('GET', '/api/drawings?symbol=' + encodeURIComponent(d.symbol) + '&period=' + encodeURIComponent(d.period));
          const match = check.drawings?.find(x => x.id === d.id && canonical(x.points) === canonical(body.points) && canonical(x.options) === canonical(body.options));
          if (match) result = { drawing: match }; else throw e;
        } else throw e;
      }
      if (!result.drawing?.digest) throw new Error('DRAWINGS_RESPONSE');
      if (identity === key(current())) {
        confirmed = confirmed.filter(x => x.id !== d.id).concat(result.drawing); replace(rows()); message(t('fundChartDrawingSaved'));
      }
      return true;
    } catch (e) {
      failed = { key: identity, draft: d, conflict: e.status === 409 };
      if (identity === key(current())) { replace(rows()); message(t(e.status === 409 ? 'fundChartDrawingConflict' : 'fundChartDrawingFailed')); }
      return false;
    } finally { busy = false; if (failed) message(t(failed.conflict ? 'fundChartDrawingConflict' : 'fundChartDrawingFailed')); }
  }
  async function remove(id) {
    if (busy || failed) return false;
    const d = confirmed.find(x => x.id === id); if (!d) return false;
    const identity = key(current()); busy = true;
    try {
      try { await api('DELETE', '/api/drawings?id=' + encodeURIComponent(id) + '&expectedDigest=' + encodeURIComponent(d.digest)); }
      catch(e) {
        if(e.status)throw e;
        const check=await api('GET','/api/drawings?symbol='+encodeURIComponent(d.symbol)+'&period='+encodeURIComponent(d.period));
        if(!Array.isArray(check.drawings) || check.drawings.some(row=>row.id===id))throw e;
      }
      if (identity === key(current())) { confirmed = confirmed.filter(x => x.id !== id); replace(rows()); message(t('fundChartDrawingDeleted')); }
      return true;
    } catch (e) {
      failed = { key: identity, draft: structuredClone(d), remove: true, conflict: e.status === 409 };
      message(t(e.status === 409 ? 'fundChartDrawingConflict' : 'fundChartDrawingFailed')); return false;
    } finally { busy = false; if (failed) message(t(failed.conflict ? 'fundChartDrawingConflict' : 'fundChartDrawingFailed')); }
  }
  function manage() {
    const identity=key(current());
    const opener=document.activeElement;
    const dialog=document.createElement('dialog');dialog.className='home-manager home-dialog drawing-manager';
    const title=document.createElement('h2'); title.textContent=t('fundChartDrawingObjects');
    const close=document.createElement('button');close.type='button';close.textContent=t('fundHomeClose');close.onclick=()=>dialog.close();
    const list=document.createElement('div');list.className='drawing-object-list';
    const form=document.createElement('form'), output=document.createElement('p');output.setAttribute('role','status');
    let draft=null;
    const field=(name,label,type,value)=>{const wrap=document.createElement('label');wrap.textContent=label;const input=document.createElement('input');input.type=type;input.name=name; if(type==='checkbox')input.checked=value;else input.value=value;wrap.append(input);form.append(wrap);return input;};
    function edit(d) {
      draft=structuredClone(d); form.replaceChildren();
      const label=document.createElement('p');label.textContent=d.toolType+' · '+d.id;form.append(label);
      field('name',t('fundChartDrawingName'),'text',d.options.name || '').maxLength=256;
      field('followTheme',t('fundChartDrawingThemeColor'),'checkbox',!d.options.color);
      field('color',t('fundChartDrawingColor'),'color',d.options.color || '#3366cc');
      const width=field('width',t('fundChartDrawingWidth'),'number',d.options.width || 1.8);width.min='0.5';width.max='10';width.step='0.1';width.required=true;
      const line=document.createElement('label');line.textContent=t('fundChartDrawingDash');const select=document.createElement('select');select.name='dash';
      for(const val of ['solid','dashed','dashed-dense','dashed-sparse']){const option=document.createElement('option');option.value=val;option.textContent=t('fundChartDrawingDash'+val);select.append(option);}select.value=d.options.dash || 'solid';line.append(select);form.append(line);
      field('visible',t('fundChartDrawingVisible'),'checkbox',d.options.visible!==false);
      field('locked',t('fundChartDrawingLocked'),'checkbox',!!d.options.locked);
      if(d.toolType==='text')field('text',t('fundTrendsDrawingText'),'text',d.options.text || '').maxLength=256;
      for(const [index,point] of d.points.entries()) {
        const date=field('date'+index,t('fundChartdate')+' '+(index+1),'text',point.date);date.maxLength=64;date.required=true;
        const price=field('price'+index,t('fundTrendsPrice')+' '+(index+1),'number',point.price);price.step='any';price.required=true;
      }
      const submit=document.createElement('button');submit.type='submit';submit.textContent=t('fundHomeSave');
      const removeButton=document.createElement('button');removeButton.type='button';removeButton.textContent=t('fundChartDrawingDelete');
      removeButton.onclick=async()=>{if(key(current())!==identity || !await Fund.home.confirm(t('fundChartDrawingDeleteConfirm')) || key(current())!==identity || !dialog.open)return;removeButton.disabled=true;if(await remove(d.id)){draft=null;form.replaceChildren();renderList();(list.querySelector('button')||close).focus();}else{removeButton.disabled=false;output.textContent=t('fundChartDrawingFailed');}};
      form.append(submit,removeButton);
    }
    function renderList(){list.replaceChildren();for(const d of confirmed){const button=document.createElement('button');button.type='button';button.textContent=(d.options.name || d.toolType)+' · '+d.id;button.onclick=()=>edit(d);list.append(button);}if(!confirmed.length)list.textContent=t('fundChartDrawingEmpty');}
    form.onsubmit=async event=>{event.preventDefault();if(!draft || key(current())!==identity)return;
      const fields=form.elements, d=structuredClone(draft);
      d.options={...d.options,name:fields.name.value.trim(),width:Number(fields.width.value),dash:fields.dash.value,visible:fields.visible.checked,locked:fields.locked.checked};
      if(fields.followTheme.checked)delete d.options.color;else d.options.color=fields.color.value;
      if(fields.text)d.options.text=fields.text.value;
      d.points=d.points.map((_,index)=>({date:fields['date'+index].value,price:Number(fields['price'+index].value)}));
      const button=form.querySelector('[type=submit]');button.disabled=true;output.textContent=t('fundChartDrawingSaving');
      const ok=await save(d);if(!dialog.open)return;
      output.textContent=t(ok?'fundChartDrawingSaved':'fundChartDrawingFailed');button.disabled=false;
      if(ok){draft=structuredClone(confirmed.find(x=>x.id===d.id));renderList();}
    };
    dialog.append(title,close,list,form,output);document.body.append(dialog);renderList();dialog.showModal();close.focus();
    const invalidate=()=>dialog.close();document.addEventListener('fund:research-selection-changed',invalidate);document.addEventListener('fund:detail-selection-changed',invalidate);
    dialog.addEventListener('close',()=>{document.removeEventListener('fund:research-selection-changed',invalidate);document.removeEventListener('fund:detail-selection-changed',invalidate);dialog.remove();if(opener?.isConnected)opener.focus({preventScroll:true});},{once:true});
  }
  return { load, save, remove, manage, canEdit: () => !busy && !failed, pause: () => { reading?.abort(); ++generation; } };
};
})();
