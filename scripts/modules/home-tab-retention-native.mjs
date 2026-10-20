// Isolated real Native owner; compares identity, never exports ledger contents.
import assert from 'node:assert/strict';
export async function verifyPositionsLoading(page,context,frame,report){
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await runtime.waitForFunction(()=>Fund.home.snapshot()!=null);
 await runtime.evaluate(async()=>{
  const asOf=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Positions loading fixture',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Positions fixture stock',quantity:'10',totalCost:'100',asOf});
  Fund.home.applyDisplayMode('amount',false);
  window.ledgerReads=0;window.delayFirstLedger=true;const request=fetch;
  window.fetch=async(url,options)=>{
   const ledger=String(url).endsWith('/api/positions/detail');if(ledger)ledgerReads++;
   const response=await request(url,options);
   if(ledger&&delayFirstLedger){delayFirstLedger=false;await new Promise((resolve,reject)=>{
    const abort=()=>reject(new DOMException('Cancelled','AbortError'));
    window.releaseLedgerRead=()=>{options.signal.removeEventListener('abort',abort);resolve();};
    if(options.signal.aborted)abort();else options.signal.addEventListener('abort',abort,{once:true});
   });}
   return response;
  };
 });
 const other=await context.newPage();await other.goto('data:text/html,<title>Positions visibility fixture</title>');await page.bringToFront();
 try{
  await frame.locator('#tab-positions').click();await runtime.waitForFunction(()=>!!window.releaseLedgerRead);
  assert.equal(await frame.locator('#holding-add').isDisabled(),true);
  await other.bringToFront();await runtime.waitForFunction(()=>document.hidden);
  await page.bringToFront();await runtime.waitForFunction(()=>!document.hidden);
  await frame.locator('.pos-holding-card:not(.pos-add-card)').waitFor({timeout:30000});
  assert.equal(await runtime.evaluate(()=>ledgerReads),2,'real initial read resumes exactly once after actual browser visibility change');
  assert.equal(await frame.locator('.pos-management').evaluate(node=>node.tagName),'SECTION');
  assert.match(await frame.locator('#pos-management-title').innerText(),/账户管理与持仓汇总|Account management/);
  for(const id of ['pos-total-cost','pos-total-day-pnl','pos-accounts','pos-refresh'])assert.equal(await frame.locator('#'+id).isVisible(),true);
  assert.equal(await frame.locator('#pos-total-cost').innerText(),'100.00');
  const reads=await runtime.evaluate(()=>ledgerReads);
  await other.bringToFront();await runtime.waitForFunction(()=>document.hidden);await page.waitForTimeout(2500);
  await page.bringToFront();await runtime.waitForFunction(()=>!document.hidden);await page.waitForTimeout(200);
  assert.equal(await runtime.evaluate(()=>ledgerReads),reads,'ready holdings are retained, rather than reloaded');
  assert.equal(await frame.locator('.pos-holding-card:not(.pos-add-card)').count(),1);
  report.steps.push({name:'real isolated ledger first read interrupted by actual tab switch resumes; translated management summary and 100.00 cost; ready ledger retention',ledgerReads:reads});
 }finally{await other.close();}
}
export async function verifyNativeTabRetention(page,context,report,extensionId){
 const other=await context.newPage();await other.goto('data:text/html,<title>Tab retention fixture</title>');
 for(const entry of ['app.html?app=fund','space.html?app=fund']){
  if(!page.url().endsWith(entry)){await page.goto('about:blank');await page.waitForTimeout(700);await page.goto(`chrome-extension://${extensionId}/${entry}`);}
  await page.bringToFront();
  const frame=page.frameLocator('iframe');await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
  if(entry.startsWith('space')){
   await page.evaluate(()=>{const channel=new BroadcastChannel('natives-workspace');channel.postMessage({type:'revision-invalidated'});channel.close();});
   await page.waitForTimeout(300);
   assert.equal(await page.locator('#space-empty-state').isVisible(),false,'workspace refresh must not cover the embedded investment view');
  }
  const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
  await runtime.waitForFunction(()=>Fund.home.snapshot()!=null&&document.getElementById('home-today-body').getAttribute('aria-busy')==='false');
  await page.waitForTimeout(500);
  await frame.locator('#home-finance-details').evaluate(el=>el.open=true);
  await frame.locator('[data-range=custom]').click();await frame.locator('#home-range-from').fill('2026-09-01');
  await page.evaluate(()=>{window.retainedIframe=document.querySelector('iframe');});
  await runtime.evaluate(()=>{window.retention={snapshot:Fund.home.snapshot(),node:document.getElementById('home-goal-body').firstElementChild};});
  const reads=[];let navigations=0;
  const request=req=>{if(/\/api\/home\/snapshot|\/api\/check\/run|\/api\/market\/indices/.test(req.url()))reads.push(new URL(req.url()).pathname);};
  const navigate=()=>navigations++;
  page.on('request',request);page.on('framenavigated',navigate);
  const starts=await page.evaluate(()=>nativeTimings.filter(row=>row.method==='app:start').length);
  for(let i=0;i<2;i++){
   await other.bringToFront();await page.waitForFunction(()=>document.hidden);await runtime.waitForFunction(()=>document.hidden);
   await page.waitForTimeout(2500);await page.bringToFront();await runtime.waitForFunction(()=>!document.hidden);await page.waitForTimeout(200);
  }
  const facts=await runtime.evaluate(()=>({sameSnapshot:retention.snapshot===Fund.home.snapshot(),sameGoal:retention.node===document.getElementById('home-goal-body').firstElementChild,draft:document.getElementById('home-range-from').value,expanded:document.getElementById('home-finance-details').open}));
  assert.equal(await page.evaluate(()=>retainedIframe===document.querySelector('iframe')),true,'iframe owner survives short browser-tab switches');
  assert.equal(await page.evaluate(()=>nativeTimings.filter(row=>row.method==='app:start').length),starts,'Runtime is not restarted');
  assert.equal(navigations,0);assert.deepEqual(reads,[]);assert.equal(facts.sameSnapshot,true);assert.equal(facts.sameGoal,true);assert.equal(facts.draft,'2026-09-01');assert.equal(facts.expanded,true);
  page.off('request',request);page.off('framenavigated',navigate);
  report.steps.push({name:'short browser-tab retention',entry,cycles:2,reads,navigations,...facts});
  await frame.locator('#home-refresh-btn').click();
  await runtime.waitForFunction(()=>retention.snapshot!==Fund.home.snapshot());
  report.steps.push({name:'explicit manual refresh still updates snapshot',entry});
 }
 await other.close();
}
