// One date-range draft, two clicks, one query. Financial values remain owned by Home.
(() => {
const {t,actions,state}=Fund;
const pane=document.getElementById('home-custom-range');
const trigger=document.querySelector('#hc-return [data-range="custom"]');
const buttons=()=>document.querySelectorAll('#hc-return [data-range]');
let selection={key:'month',from:'',to:''},start='',end='',month='',returnFocus=false;
const iso=d=>d.toISOString().slice(0,10);
const day=d=>new Date(d+'T12:00:00Z');
const shift=(d,n)=>{const date=day(d);date.setUTCDate(date.getUTCDate()+n);return iso(date);};
const today=()=>Fund.home?.snapshot()?.requestedRange?.today||iso(new Date(Date.now()+8*3600000));
const label=d=>day(d).toLocaleDateString(state.language,{dateStyle:'medium',timeZone:'UTC'});
const escape=v=>String(v).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const monthShift=(value,n)=>{const d=day(value+'-01');d.setUTCMonth(d.getUTCMonth()+n);return iso(d).slice(0,7);};
function place(){
 if(!pane.matches(':popover-open'))return;
 const r=trigger.getBoundingClientRect(),p=pane.getBoundingClientRect();
 if(r.bottom<=0||r.top>=innerHeight){close();return;}
 pane.style.left=Math.max(12,Math.min(r.right-p.width,innerWidth-p.width-12))+'px';
 pane.style.top=Math.max(12,Math.min(r.bottom+8,innerHeight-p.height-12))+'px';
}
function preview(value=end||start){
 pane.querySelector('[data-range-preview]').textContent=start?label(start)+(value&&value!==start?' -- '+label(value):''):t('fundHomeRangePickStart');
 pane.querySelector('[data-range-instruction]').textContent=t(start&&!end?'fundHomeRangePickEnd':'fundHomeRangePickStart');
 pane.querySelectorAll('[data-date]').forEach(b=>{
   const picked=b.dataset.date===start||b.dataset.date===end;
   b.setAttribute('aria-selected',String(picked));b.classList.toggle('range-edge',picked);
   b.classList.toggle('range-between',!!start&&b.dataset.date>start&&b.dataset.date<(value||start));
 });
 place();
}
function render(){
 const limit=today(),locale=state.language,year=Number(month.slice(0,4)),m=Number(month.slice(5));
 const monday=shift(limit,-((day(limit).getUTCDay()+6)%7));
 const weekdays=Array.from({length:7},(_,i)=>day(shift(monday,i)).toLocaleDateString(locale,{weekday:innerWidth<560?'narrow':'short',timeZone:'UTC'}));
 const calendar=value=>{
   const first=value+'-01',offset=(day(first).getUTCDay()+6)%7;
   const cells=Array.from({length:42},(_,i)=>{
     const date=shift(first,i-offset),outside=date.slice(0,7)!==value;
     return `<button type="button" role="gridcell" data-date="${date}" class="range-day${outside?' range-outside':''}" tabindex="${date===(start||limit)?0:-1}" aria-label="${escape(day(date).toLocaleDateString(locale,{dateStyle:'full',timeZone:'UTC'}))}" ${date>limit?'disabled':''} ${date===limit?'aria-current="date"':''}>${Number(date.slice(8))}</button>`;
   });
   return `<section class="range-month"><h4>${escape(day(first).toLocaleDateString(locale,{month:'long',year:'numeric',timeZone:'UTC'}))}</h4><div class="range-weekdays">${weekdays.map(w=>`<span>${escape(w)}</span>`).join('')}</div><div class="range-grid" role="grid">${Array.from({length:6},(_,i)=>`<div role="row">${cells.slice(i*7,i*7+7).join('')}</div>`).join('')}</div></section>`;
 };
 pane.innerHTML=`<header><div><strong>${escape(t('fundHomeCustom'))}</strong><small data-range-instruction></small></div><button type="button" data-range-close aria-label="${escape(t('fundHomeClose'))}">×</button></header>
 <div class="range-month-nav"><button type="button" data-month-step="-1" aria-label="${escape(t('fundHomeRangePreviousMonth'))}">‹</button><select data-range-month aria-label="${escape(t('fundHomeRangeMonth'))}">${Array.from({length:12},(_,i)=>`<option value="${i+1}" ${m===i+1?'selected':''}>${escape(day(`${limit.slice(0,4)}-${String(i+1).padStart(2,'0')}-01`).toLocaleDateString(locale,{month:'long',timeZone:'UTC'}))}</option>`).join('')}</select><input data-range-year type="number" min="1900" max="${Number(limit.slice(0,4))}" value="${year}" aria-label="${escape(t('fundHomeRangeYear'))}"><button type="button" data-month-step="1" aria-label="${escape(t('fundHomeRangeNextMonth'))}" ${month>=limit.slice(0,7)?'disabled':''}>›</button></div>
 <div class="range-calendars">${calendar(month)}${calendar(monthShift(month,1))}</div><footer><span data-range-preview></span><small>${escape(t('fundHomeRangeAutoApply'))}</small></footer>`;
 preview();place();
}
function open(){
 start='';end='';month=innerWidth<560?today().slice(0,7):monthShift(today().slice(0,7),-1);
 render();pane.showPopover();place();
 pane.querySelector(`[data-date="${today()}"]:not(.range-outside)`)?.focus();
}
function close(focus=false){returnFocus=focus;if(pane.matches(':popover-open'))pane.hidePopover();}
pane.addEventListener('toggle',e=>{trigger.setAttribute('aria-expanded',String(e.newState==='open'));if(e.newState==='closed'&&returnFocus){returnFocus=false;trigger.focus();}});
pane.addEventListener('click',e=>{
 if(e.target.closest('[data-range-close]')){close(true);return;}
 const nav=e.target.closest('[data-month-step]');if(nav){month=monthShift(month,Number(nav.dataset.monthStep));render();return;}
 const cell=e.target.closest('[data-date]');if(!cell||cell.disabled)return;
 const date=cell.dataset.date;
 if(!start||end||date<start){start=date;end='';preview();return;}
 end=date;close(true);actions.setHomeRange('custom',start,end);
});
pane.addEventListener('pointerover',e=>{const d=e.target.closest('[data-date]');if(start&&!end&&d&&!d.disabled&&d.dataset.date>=start)preview(d.dataset.date);});
pane.addEventListener('change',e=>{
 if(!e.target.matches('[data-range-month],[data-range-year]'))return;
 const y=Number(pane.querySelector('[data-range-year]').value),m=Number(pane.querySelector('[data-range-month]').value);
 if(!Number.isInteger(y)||y<1900||y>Number(today().slice(0,4)))return;
 month=`${y}-${String(m).padStart(2,'0')}`;if(month>today().slice(0,7))month=today().slice(0,7);render();
});
pane.addEventListener('keydown',e=>{
 e.stopPropagation();if(e.key==='Escape'){returnFocus=true;return;}
 const cell=e.target.closest('[data-date]');if(!cell)return;
 const delta={ArrowLeft:-1,ArrowRight:1,ArrowUp:-7,ArrowDown:7}[e.key];if(delta==null)return;e.preventDefault();
 const next=shift(cell.dataset.date,delta);if(next>today())return;
 if(!pane.querySelector(`[data-date="${next}"]:not(.range-outside)`)){month=next.slice(0,7);render();}
 pane.querySelectorAll('[data-date]').forEach(b=>b.tabIndex=-1);
 const target=pane.querySelector(`[data-date="${next}"]:not(.range-outside)`);if(target){target.tabIndex=0;target.focus();}
});
buttons().forEach(button=>button.addEventListener('click',()=>{
 if(button.dataset.range==='custom'){open();return;}close();actions.setHomeRange(button.dataset.range);
}));
const update=data=>{
 if(data)selection=data;
 buttons().forEach(b=>{const selected=b.dataset.range===selection.key;b.classList.toggle('active',selected);b.setAttribute('aria-pressed',String(selected));});
 const range=document.getElementById('home-selected-range');
 if(range)range.textContent=selection.from?label(selection.from)+' -- '+label(selection.to):t('fundHomeSinceTrackingBegan');
};
document.addEventListener('fund:language-changed',()=>{update();if(pane.matches(':popover-open'))render();});
window.addEventListener('resize',place);window.addEventListener('scroll',place,true);
window.addEventListener('pagehide',()=>{close();window.removeEventListener('resize',place);window.removeEventListener('scroll',place,true);});
window.addEventListener('pageshow',e=>{if(e.persisted){window.addEventListener('resize',place);window.addEventListener('scroll',place,true);}});
Fund.homeRange={update,close};update();
})();
