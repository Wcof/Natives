// Real broker and Runtime, isolated ledger, explicitly synthetic information/provider.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {join} from 'node:path';
import {resolve} from 'node:path';

export async function verifyInlineAI({page,frame,report,fakeHome,out,env,extensionId}) {
 let runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await runtime.evaluate(async()=>{
  const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await Fund.api('POST','/api/openings',{account:'Synthetic AI account',instrumentKind:'stock',market:'SSE',fundCode:'600519',fundName:'Synthetic equity',quantity:'10',totalCost:'100',asOf:today});
 });
 const pid=JSON.parse(readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').filter(line=>JSON.parse(line).host==='com.natives.local.app_runtime').at(-1)).pid;
 await page.goto('about:blank');
 const until=performance.now()+2000;
 while(performance.now()<until){try{process.kill(pid,0);}catch{break;}await new Promise(resolve=>setTimeout(resolve,20));}
 assert.throws(()=>process.kill(pid,0),'Runtime must stop before isolated fixture setup');
 const seed=spawnSync('python3',['-c',`
import sqlite3,sys,datetime,json
c=sqlite3.connect(sys.argv[1]);stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ');ids=[]
for suffix in ['A','B']:
    title='Synthetic portfolio notice '+suffix
    cur=c.execute("INSERT INTO information_items(source_id,external_id,title,published_at,content_text,completeness,time_precision,retrieved_at,content_hash,event_type,importance,created_at,updated_at) VALUES('em_stock_ann',?,?,?,?,'excerpt','second',?,?,'notice',10,?,?)",('synthetic-ai-'+suffix,title,stamp,'Synthetic evidence body '+suffix+'; synthetic detail\\n'*80,stamp,'synthetic-hash-'+suffix,stamp,stamp))
    ids.append(cur.lastrowid)
    c.execute("INSERT INTO information_relations(item_id,instrument_ref,relation,reason,created_at) VALUES(?,'security:CN:stock:600519','direct','isolated synthetic relation',?)",(cur.lastrowid,stamp))
c.commit();c.close();print(json.dumps(ids))
`,join(fakeHome,'.natives-local/apps/fund/data/fund.db')],{env,encoding:'utf8'});
 assert.equal(seed.status,0,seed.stderr);const [first,second]=JSON.parse(seed.stdout);
 await page.goto(`chrome-extension://${extensionId}/app.html?app=fund`);await page.bringToFront();
 await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
 runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await runtime.waitForFunction(()=>Fund.home.snapshot()!=null);
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 await frame.locator(`[data-act=news-ai][data-id="${first}"]`).click();
 await frame.locator('[data-act=home-ai-confirm]:not([disabled])').waitFor({timeout:15000});
 assert.match(await frame.locator('#home-ai-body').innerText(),/Synthetic evidence body A/);
 assert.equal(report.providerRequests.length,0,'preview must not send inference');
 await frame.locator('[data-act=home-ai-confirm]').click();
 await frame.locator('[data-act=home-ai-record]').waitFor({timeout:30000});
 assert.deepEqual(report.providerRequests[0].informationIds,['I'+first]);
 assert.equal(report.providerRequests[0].disclosedFinance,false);
 assert.equal(report.providerRequests[0].syntheticAccountIncluded,false);
 assert.equal(await frame.locator('#view-home').isVisible(),true);
 let releaseSave,saveReady,saveRequests=0;
 const savedOnServer=new Promise(resolve=>{saveReady=resolve;}),release=new Promise(resolve=>{releaseSave=resolve;});
 await page.route('**/api/analysis/reports/save',async route=>{saveRequests++;const response=await route.fetch();saveReady();await release;await route.fulfill({response});});
 await frame.locator('[data-act=home-ai-save]').click();
 await savedOnServer;
 await frame.locator('[data-act=home-ai-record]').click();
 const note='Synthetic decision: observe pending evidence; private amount 12345.67';
 await frame.locator('dialog[open] [name=note]').fill(note);
 await frame.locator('dialog[open] [type=submit]').click();
 assert.equal(saveRequests,1,'a decision during an outstanding save must share the same report');
 releaseSave();
 await frame.locator('dialog[open]').waitFor({state:'detached'});
 await page.unroute('**/api/analysis/reports/save');
 const saved=await runtime.evaluate(async()=>({decisions:await Fund.api('GET','/api/information/decisions'),reports:await Fund.api('GET','/api/analysis/reports?page=1'),trades:await Fund.api('GET','/api/transactions')}));
 assert.equal(saved.decisions.records.length,1);assert.equal(saved.reports.reports.length,1);
 assert.equal(saved.decisions.records[0].informationId,first);
 assert.equal(saved.decisions.records[0].reportId,saved.reports.reports[0].id);
 assert.equal(saved.decisions.records[0].action,'observe');
 assert.deepEqual(saved.trades.transactions,[],'saving a report or decision must never create a trade');
 assert.equal(await frame.locator('#view-home').isVisible(),true);
 const sent=report.providerRequests.length;
 await frame.locator('#home-history').click();
 await frame.locator('#view-history').getByText('Synthetic decision: observe pending evidence',{exact:false}).waitFor();
 await frame.locator(`[data-decision-report="${saved.decisions.records[0].reportId}"] summary`).click();
 await frame.locator('#view-history [data-saved-report]').getByText('Synthetic evidence body A',{exact:false}).waitFor();
 assert.match(await frame.locator('#view-history [data-saved-report]').innerText(),/Synthetic full-chain explanation/);
 assert.equal(report.providerRequests.length,sent,'history must not send new inference');
 await frame.locator('#tab-home').click();
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('percent',false));
 assert.ok(!(await frame.locator('#home-ai-body').innerText()).includes('12345.67'),'percentage mode must conceal free-text decision notes');
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
 assert.ok(!(await frame.locator('#home-ai-body').innerText()).includes('Synthetic portfolio notice A'));
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 await frame.locator(`[data-act=news-ai][data-id="${second}"]`).focus();
 await frame.locator(`[data-act=news-ai][data-id="${second}"]`).press('Enter');
 await frame.locator('[data-act=home-ai-confirm]:not([disabled])').waitFor();
 assert.match(await frame.locator('#home-ai-body').innerText(),/Synthetic evidence body B/);
 const snapshots=resolve(import.meta.dirname,'../../.tmp/investment-final-fixes/inline-ai');mkdirSync(snapshots,{recursive:true});
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1440,768,320]){
  await page.setViewportSize({width,height:900});
  await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));},{theme,language});
  assert.doesNotMatch(await frame.locator('.home-decision-grid').innerText(),/fundHome[A-Z]|fundDetail[A-Z]/);
  assert.equal(await runtime.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  const sizes=await frame.locator('.home-decision-grid').evaluate(el=>[...el.querySelectorAll('.home-news-panel,.home-ai-panel')].map(card=>card.getBoundingClientRect().height));
  assert.ok(Math.abs(sizes[0]-sizes[1])<1,'information and AI cards must retain equal fixed heights');
  await frame.locator('#home-ai-body').focus();await frame.locator('#home-ai-body').press('End');
  await runtime.waitForFunction(()=>document.getElementById('home-ai-body').scrollTop>0,null,{timeout:1500});
  assert.ok(await frame.locator('#home-ai-body').evaluate(el=>el.scrollHeight>el.clientHeight&&el.scrollTop>0),'long preview scrolls with keyboard');
  await frame.locator('.home-decision-grid').screenshot({path:join(snapshots,`inline-${theme}-${language}-${width}.png`)});
 }
 const metrics=await page.context().newCDPSession(page);await metrics.send('Emulation.clearDeviceMetricsOverride');await metrics.detach();
 const configPath=join(env.NATIVES_MODEL_HOST_CONFIG_DIR,'state.json');assert.ok(configPath.startsWith(out));
 const defaultState=JSON.parse(readFileSync(configPath,'utf8'));
 const stopPage=async()=>{
  const pids=new Map(readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').map(line=>JSON.parse(line)).map(row=>[row.host,row.pid]));
  await page.goto('about:blank');const deadline=performance.now()+2000;
  const live=()=>[...pids.values()].some(pid=>{try{process.kill(pid,0);return true;}catch{return false;}});
  while(live()&&performance.now()<deadline)await new Promise(resolve=>setTimeout(resolve,20));
  assert.equal(live(),false,'isolated Hosts must stop before editing their fixture configuration');
 };
 await stopPage();writeFileSync(configPath,JSON.stringify({...defaultState,defaultModel:{},revision:defaultState.revision+1}));
 await page.goto(`chrome-extension://${extensionId}/app.html?app=fund`);await page.bringToFront();await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
 runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));await runtime.waitForFunction(()=>Fund.home.snapshot()!=null);await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 await frame.locator(`[data-act=news-ai][data-id="${second}"]`).focus();
 await frame.locator(`[data-act=news-ai][data-id="${second}"]`).press('Enter');
 await runtime.waitForFunction(id=>Fund.home.isNewsAISelected(id),second,{timeout:5000});
 await frame.locator('[data-act=home-ai-confirm]:not([disabled])').waitFor();
 await frame.locator('[data-act=home-ai-confirm]').focus();await frame.locator('[data-act=home-ai-confirm]').press('Enter');
 await frame.locator('.home-ai-error').waitFor();
 assert.equal(report.providerRequests.length,sent,'an unset default must not fall back to the first enabled model');
 assert.ok((await frame.locator('.home-ai-error').innerText()).includes(await runtime.evaluate(()=>Fund.t('fundHomeAiDefaultUnavailable'))));
 await frame.locator('[data-act=home-ai-model-settings]').focus();await frame.locator('[data-act=home-ai-model-settings]').press('Enter');
 await page.locator('.model-settings-dialog[open]').waitFor({timeout:15000});await page.keyboard.press('Escape');
 await page.locator('.model-settings-dialog').waitFor({state:'hidden'});
 await runtime.waitForFunction(()=>document.activeElement?.matches('[data-act=home-ai-model-settings]'),null,{timeout:5000});
 report.steps.push({name:'Inline evidence preview → explicit broker inference → report → real information decision → history; selection switches to B',first,second,decision:saved.decisions.records[0],tradeResult:saved.trades});
 report.steps.push({name:'12 fixed-height theme/language/viewport cases with keyboard scrolling; unset default returns an error without implicit provider use',saveRequests});
}
