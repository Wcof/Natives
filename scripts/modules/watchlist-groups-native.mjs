// Real Chrome → Native → isolated ledger; no user data or external quotes.
import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
export async function verifyNativeWatchlistGroups(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 const out=resolve(import.meta.dirname,'../../.tmp/watchlist-groups');mkdirSync(out,{recursive:true});
 assert.equal(await runtime.evaluate(()=>Fund.state.activeGroup),'','initial view is All');
 await runtime.evaluate(async()=>{
  await Fund.api('POST','/api/watchlists',{id:'group-test',name:'Long group name for wrapping 分组'});
  for(const [watchlistId,symbol] of [['default','jj000001'],['group-test','jj000002'],['group-test','jj000001']])await Fund.api('POST','/api/watchlists/items',{watchlistId,symbol,name:symbol,assetType:'fund'});
  await Fund.actions.refreshWatchlist();
 });
 await frame.locator('#tab-watchlist').click();
 await frame.locator('#watchlist-table tr[data-symbol]').nth(1).waitFor();
 assert.equal(await frame.locator('#watchlist-table tr[data-symbol]').count(),2,'All deduplicates cross-group instruments');
 for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [1280,320]){
  await page.setViewportSize({width,height:900});await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
  await runtime.waitForFunction(({theme,language})=>document.documentElement.dataset.theme===theme&&Fund.state.language===(language==='en'?'en':'zh-CN'),{theme,language});
  const all=frame.locator('#watchlist-groups [data-id=""]');await all.click();
  assert.equal(await all.innerText(),language==='en'?'All':'全部');assert.equal(await all.getAttribute('aria-selected'),'true');
  await all.focus();await all.press('End');await runtime.waitForFunction(()=>Fund.state.activeGroup==='group-test');
  await frame.locator('#watchlist-groups [data-id=group-test]').press('Home');await runtime.waitForFunction(()=>Fund.state.activeGroup==='');
  await runtime.waitForFunction(()=>document.querySelector('#watchlist-groups [data-id=""]').getAttribute('aria-selected')==='true');
  assert.equal(await all.evaluate(el=>el===document.activeElement),true,'refresh preserves All tab focus: '+await runtime.evaluate(()=>JSON.stringify({active:document.activeElement?.outerHTML,group:Fund.state.activeGroup,visible:document.visibilityState})));
  const layout=await runtime.evaluate(()=>{
   const nav=document.getElementById('watchlist-groups'),tools=document.querySelector('.watchlist-tools');
   return {gap:getComputedStyle(nav).gap,nav:nav.getBoundingClientRect().bottom,tools:tools.getBoundingClientRect().top,overflow:document.documentElement.scrollWidth>document.documentElement.clientWidth+1,heights:[...nav.children].map(el=>el.getBoundingClientRect().height)};
  });
  assert.equal(layout.gap,'8px');assert.ok(layout.tools>layout.nav);assert.equal(layout.overflow,false);assert.ok(layout.heights.every(h=>h>=36));
  await frame.locator('#watchlist-groups').scrollIntoViewIfNeeded();await page.screenshot({path:resolve(out,`${theme}-${language}-${width}.png`)});
 }
 await runtime.evaluate(()=>Fund.actions.setWatchlistMember({symbol:'jj000003',name:'Added in All',assetType:'fund'},true));
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.find(it=>it.symbol==='jj000003').watchlistId,'default','All adds to actual default group');
 await frame.locator('#watchlist-groups [data-id=group-test]').click();await runtime.waitForFunction(()=>Fund.state.activeGroup==='group-test');
 await frame.locator('#watchlist-table tr[data-symbol=jj000001] .btn-del').click();
 await runtime.waitForFunction(()=>!Fund.state.watchlistItems.some(it=>it.symbol==='jj000001'));
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.filter(it=>it.symbol==='jj000001').length,1,'group removal preserves other membership');
 await runtime.evaluate(async()=>{await Fund.api('POST','/api/watchlists/items',{watchlistId:'group-test',symbol:'jj000001',name:'Duplicate',assetType:'fund'});await Fund.actions.refreshWatchlist();});
 await frame.locator('#watchlist-groups [data-id=""]').click();await runtime.waitForFunction(()=>Fund.state.activeGroup==='');
 await frame.locator('#watchlist-table tr[data-symbol=jj000001] .btn-del').click();await runtime.waitForFunction(()=>!Fund.state.watchlistItems.some(it=>it.symbol==='jj000001'));
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.filter(it=>it.symbol==='jj000001').length,0,'All removal clears all matching memberships');
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 await frame.locator('#watchlist-manage-group').click();assert.equal(await frame.locator('dialog[open] [name=id]').inputValue(),'default');await frame.locator('dialog[open]').press('Escape');
 const before=await runtime.evaluate(()=>Fund.state.watchlistItems.map(it=>it.symbol));
 await page.route('**/api/watchlists',route=>route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:'TEST_UNAVAILABLE'})}));
 await runtime.evaluate(()=>Fund.actions.refreshWatchlist());assert.deepEqual(await runtime.evaluate(()=>Fund.state.watchlistItems.map(it=>it.symbol)),before);
 await frame.locator('#watchlist-status button').waitFor();await page.unroute('**/api/watchlists');await frame.locator('#watchlist-status button').click();
 await runtime.waitForFunction(()=>document.getElementById('watchlist-status').textContent==='');
 await verifyNativeWatchlistMoves(page,frame,report);
 report.steps.push({name:'Default All, dedup/filter, group and All removal, add/management use actual group, keyboard/focus, retry retaining data; 8 theme/language/width combinations',screenshots:out});
}

async function verifyNativeWatchlistMoves(page,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 const posted=[];page.on('request',r=>{if(r.url().includes('/api/watchlists/manage')&&r.method()==='POST')posted.push(JSON.parse(r.postData()));});
 // Synthetic public quotes only; membership mutations below use the real isolated DB.
 await page.route('**/api/market/quotes?*',route=>route.fulfill({contentType:'application/json',body:JSON.stringify({quotes:[{code:'sh600519',price:1,asOf:new Date().toISOString()}]})}));
 await runtime.evaluate(async()=>{
  Fund.home.applyDisplayMode('amount',false);
  for(const id of ['move-a','move-b','move-c'])await Fund.api('POST','/api/watchlists',{id,name:id});
  for(const [symbol,assetType] of [['sh600519','stock'],['jj000004','fund']])await Fund.api('POST','/api/watchlists/items',{watchlistId:'move-a',symbol,name:symbol,assetType});
  await Fund.actions.refreshWatchlist();
 });
 const choose=async id=>{await frame.locator('#watchlist-groups [data-id="'+id+'"]').click();await runtime.waitForFunction(id=>Fund.state.activeGroup===id&&document.querySelector('#watchlist-groups [aria-selected=true]')?.dataset.id===id,id);};
 await choose('move-a');await frame.locator('[data-watch-move=sh600519]').waitFor();
 const single=frame.locator('[data-watch-move=sh600519]');
 for(const theme of ['archive','volt'])for(const language of ['zh_CN','en'])for(const width of [1280,320]){
  await page.setViewportSize({width,height:900});await page.evaluate(({theme,language})=>chrome.storage.local.set({'natives-theme':theme,'natives-language':language}),{theme,language});
  await runtime.waitForFunction(({theme,language})=>document.documentElement.dataset.theme===theme&&Fund.state.language===(language==='en'?'en':'zh-CN'),{theme,language});
  await single.focus();await single.press('Enter');await frame.locator('dialog[open] [data-watch-move]').waitFor();
  const submit=frame.locator('dialog[open] [type=submit]');assert.equal(await submit.isDisabled(),true);
  assert.equal(await frame.locator('dialog[open] select').count(),1,'only one target selector');
  assert.equal(await frame.locator('dialog[open] [data-move-source]').count(),0);
  assert.equal(await frame.locator('dialog[open] [name=target] option[value=move-a]').count(),0,'current source is not offered as target');
  await frame.locator('dialog[open] [name=target]').selectOption('move-b');assert.equal(await submit.isEnabled(),true);
  assert.equal(await frame.locator('dialog[open]').evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
  await page.screenshot({path:resolve(import.meta.dirname,'../../.tmp/watchlist-groups',`move-${theme}-${language}-${width}.png`)});
  await frame.locator('dialog[open]').press('Escape');assert.equal(await single.evaluate(el=>document.activeElement===el),true);assert.equal(posted.length,0,'open/cancel performs no moves');
 }
 await page.setViewportSize({width:1280,height:900});
 const submitMove=async()=>{await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open]').waitFor({state:'detached'});};
 await single.click();await frame.locator('dialog[open] [name=target]').selectOption('move-b');await submitMove();
 await choose('move-b');await frame.locator('[data-watch-move=sh600519]').click();await frame.locator('dialog[open] [name=target]').selectOption('move-c');await submitMove();
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.find(it=>it.symbol==='sh600519').watchlistId,'move-c','single stock A → B → C');
 await choose('move-a');await frame.locator('[data-watch-select=jj000004]').check();await frame.locator('#watchlist-move-selected').click();
 await frame.locator('dialog[open] [name=target]').selectOption('move-c');
 await runtime.evaluate(()=>Fund.api('POST','/api/watchlists',{id:'move-concurrent',name:'Concurrent change'}));
 await frame.locator('dialog[open] [type=submit]').click();await frame.locator('dialog[open] [data-move-review]:visible').waitFor();
 assert.equal(await frame.locator('dialog[open] [name=target]').inputValue(),'move-c');assert.equal(await frame.locator('dialog[open] [type=submit]').isDisabled(),true);
 await frame.locator('dialog[open] [data-move-review]').click();await frame.locator('dialog[open] [type=submit]:enabled').waitFor();await submitMove();
 const snapshot=await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'));assert.equal(snapshot.items.find(it=>it.symbol==='jj000004').watchlistId,'move-c','single fund bulk entry');
 await choose('move-c');await frame.locator('#watchlist-select-all').check();await frame.locator('#watchlist-move-selected').click();
 await frame.locator('dialog[open] [name=target]').selectOption('move-b');await submitMove();
 const bulk=posted.at(-1);assert.equal(bulk.op,'moveSelected');assert.equal(bulk.symbols.length,2,'one atomic request moves stock and fund');assert.equal(bulk.source,'move-c');
 assert.equal((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.filter(it=>it.watchlistId==='move-b').length,2);
 await runtime.evaluate(async()=>{await Fund.api('POST','/api/watchlists/items',{watchlistId:'move-a',symbol:'jj000004',name:'Another membership',assetType:'fund'});await Fund.actions.refreshWatchlist();});
 await choose('');await frame.locator('[data-watch-move=jj000004]').click();
 assert.equal(await frame.locator('dialog[open] select').count(),1,'All single entry also chooses only a target');
 await frame.locator('dialog[open] [name=target]').selectOption('move-b');assert.equal(await frame.locator('dialog[open] [type=submit]').isEnabled(),true,'existing target merges rather than blocks');await submitMove();
 assert.deepEqual((await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'))).items.filter(it=>it.symbol==='jj000004').map(it=>it.watchlistId),['move-b']);
 // Add a target in another session without refreshing the page projection.
 await runtime.evaluate(async()=>{
  for(const symbol of ['sh600519','jj000004'])await Fund.api('POST','/api/watchlists/items',{watchlistId:'move-a',symbol,name:'Other membership',assetType:symbol.startsWith('jj')?'fund':'stock'});
  await Fund.api('POST','/api/watchlists',{id:'fresh-target',name:'Fresh target'});
  await Fund.api('POST','/api/watchlists/items',{watchlistId:'fresh-target',symbol:'sh600519',name:'Target stock metadata',assetType:'stock'});
 });
 assert.equal(await frame.locator('#watchlist-groups [data-id=fresh-target]').count(),0,'fresh target is not yet in the page projection');
 await frame.locator('[data-watch-select=sh600519]').check();await frame.locator('[data-watch-select=jj000004]').check();
 await page.route('**/api/watchlists',route=>route.fulfill({status:503,json:{error:'APP_UPSTREAM'}}));
 await frame.locator('#watchlist-move-selected').click();await frame.locator('dialog[open] [data-move-review]:visible').waitFor();
 assert.equal(await frame.locator('dialog[open] [name=target]').isDisabled(),true);
 assert.equal(await frame.locator('dialog[open] [type=submit]').isDisabled(),true,'failed fresh read cannot submit stale projection');
 await page.unroute('**/api/watchlists');await frame.locator('dialog[open] [data-move-review]').click();
 await frame.locator('dialog[open] [name=target]:enabled').waitFor();
 assert.equal(await frame.locator('dialog[open] select').count(),1,'bulk has exactly one selector for every instrument');
 assert.equal(await frame.locator('dialog[open] [name=target] option[value=fresh-target]').count(),1,'fresh target appears after opening/retry without refreshing the page');
 await frame.locator('dialog[open] [name=target]').selectOption('fresh-target');
 await runtime.evaluate(()=>Fund.applyLanguage('en'));
 assert.equal(await frame.locator('dialog[open] [name=target]').inputValue(),'fresh-target','language preserves the one chosen target');
 await submitMove();
 const final=await runtime.evaluate(()=>Fund.api('GET','/api/watchlists'));
 for(const symbol of ['sh600519','jj000004'])assert.deepEqual(final.items.filter(it=>it.symbol===symbol).map(it=>it.watchlistId),['fresh-target'],'All consolidates every selected instrument to one target');
 assert.equal(posted.at(-1).source,'');assert.equal(posted.at(-1).symbols.length,2);
 assert.equal(final.items.find(it=>it.symbol==='sh600519').name,'Target stock metadata');
 assert.equal(await frame.locator('tr[data-symbol=sh600519] [data-watch-name]').innerText(),'Target stock metadata','merge refreshes row identity as well as membership');
 await choose('fresh-target');await frame.locator('#watchlist-select-all').check();await choose('move-c');assert.equal(await frame.locator('#watchlist-move-selected').isDisabled(),true,'switching groups clears selection');
 await page.unroute('**/api/market/quotes?*');
 report.steps.push({name:'One target only: stock A→B→C, fund single/bulk, atomic mixed batch, All consolidates memberships, existing target merge/metadata, fresh groups after 503/retry, real 409/draft/review, language preserves target, 8 dialog theme/language/width/Enter/Esc/focus cases',moves:posted.length,syntheticPublicQuotes:true});
}
