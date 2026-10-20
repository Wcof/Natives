// Public metadata has its own bounded request owner; it never delays ledger reads.
(() => {
const cache=new Map(),owners=new Map(),ttl=86400000;
const hidden=()=>Fund.home?.displayMode?.()==='hidden';
function paint(root){
 const nodes=[...root.querySelectorAll('[data-fund-classification]')],allowed=new Set([...new Set(nodes.map(node=>node.dataset.fundClassification))].slice(0,20));
 for(const [index,node] of nodes.entries()){
  const focused=node.contains(document.activeElement);node.replaceChildren();node.hidden=hidden();if(node.hidden)continue;
  if(focused){node.tabIndex=-1;node.focus({preventScroll:true});}
  const code=node.dataset.fundClassification,item=cache.get(code);
  if(!allowed.has(code)){node.textContent=Fund.t('fundDetailThemeLimit');continue;}
  if(!item||Date.now()-item.at>=ttl){node.textContent=Fund.t('fundDetailThemeLoading');continue;}
  if(item.error){const text=document.createElement('span');text.textContent=Fund.t('fundDetailThemeFailed');const button=document.createElement('button');button.type='button';button.id=`${root.id}-theme-retry-${index}-${code}`;button.textContent=Fund.t('fundLedgerRetry');button.onclick=()=>{cache.delete(code);cancel(root);mount(root);};node.append(text,button);if(focused)button.focus({preventScroll:true});continue;}
  const data=item.data,labels=(data.themes||[]).map(tag=>tag.name);
  if(!labels.length&&data.indexName)labels.push(Fund.t('fundDetailThemeIndex',{name:data.indexName}));
  if(!labels.length){node.textContent=Fund.t('fundDetailThemeUnavailable');continue;}
  for(const label of labels){const tag=document.createElement('span');tag.className='fund-theme-tag';tag.textContent=label;node.append(tag);}
  const source=document.createElement('a');source.id=`${root.id}-theme-source-${index}-${code}`;source.href=data.sourceUrl;source.target='_blank';source.rel='noopener noreferrer';source.textContent=Fund.t('fundDetailThemeSource');
  source.title=Fund.t('fundDetailThemeAsOf',{date:new Date(data.fetchedAt*1000).toLocaleString(Fund.state.language,{timeZone:'Asia/Shanghai'})});node.append(source);if(focused)source.focus({preventScroll:true});
 }
}
function cancel(root){const owner=owners.get(root);owner?.controller.abort();owners.delete(root);}
function mount(root){
 if(!root)return;
 if(root.hidden||document.hidden||hidden()){cancel(root);paint(root);return;}
 // shortcut: read at most 20 visible funds; add metadata paging for larger portfolios.
 const codes=[...new Set([...root.querySelectorAll('[data-fund-classification]')].map(node=>node.dataset.fundClassification).filter(code=>/^\d{6}$/.test(code)))].slice(0,20);
 const key=codes.join(','),current=owners.get(root);paint(root);
 if(current?.key===key&&(current.loading||codes.every(code=>{const item=cache.get(code);return item&&Date.now()-item.at<ttl;})))return;
 cancel(root);const owner={key,controller:new AbortController(),loading:true};owners.set(root,owner);
 const queue=codes.filter(code=>{const item=cache.get(code);return !item||Date.now()-item.at>=ttl;});
 async function worker(){while(queue.length&&!owner.controller.signal.aborted){const code=queue.shift();try{
  const data=await Fund.api('GET','/api/fund/classification?code='+code,null,{signal:owner.controller.signal});
  if(owner.controller.signal.aborted)break;
  if(data.code!==code||!['ready','unclassified'].includes(data.status)||!Array.isArray(data.themes)||!/^https:\/\/fund(?:mobapi)?\.eastmoney\.com\//.test(data.sourceUrl)||!Number.isFinite(data.fetchedAt))throw new Error('Invalid classification response');
  if(cache.size>=100&&!cache.has(code))cache.delete(cache.keys().next().value);
  cache.set(code,{at:Date.now(),data});
 }catch(error){if(owner.controller.signal.aborted||error.name==='AbortError')break;cache.set(code,{at:Date.now(),error:true});}
 if(owners.get(root)===owner&&!root.hidden&&!document.hidden)paint(root);
 }}
 void Promise.all(Array.from({length:Math.min(3,queue.length)},worker)).finally(()=>{owner.loading=false;});
}
document.addEventListener('visibilitychange',()=>{for(const root of [...owners.keys()]){if(document.hidden)cancel(root);else mount(root);}if(!document.hidden)for(const id of ['view-positions','view-fund-detail']){const root=document.getElementById(id);if(root&&!root.hidden)mount(root);}});
window.addEventListener('pagehide',()=>{for(const root of [...owners.keys()])cancel(root);});
Fund.fundClassification={mount,cancel};
})();
