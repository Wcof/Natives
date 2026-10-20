// Actual Native chain and isolated persisted ledger; never opens the user's DB.
import assert from 'node:assert/strict';
import {readFileSync,mkdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
export async function verifyNativeHomeRanges({page,frame,report,fakeHome,out,env,extensionId}) {
 const pid=readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').map(JSON.parse).filter(r=>r.host==='com.natives.local.app_runtime').at(-1).pid;
 await page.goto('about:blank');
 const deadline=Date.now()+2000;
 while(Date.now()<deadline){try{process.kill(pid,0);}catch{break;}await new Promise(r=>setTimeout(r,20));}
 assert.throws(()=>process.kill(pid,0),'seed only after Runtime EOF');
 const seed=spawnSync('python3',['-c',`
import sqlite3,datetime,json,sys,time
c=sqlite3.connect(sys.argv[1]);now=int(time.time());today=datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).date()
c.execute("INSERT INTO accounts(name,created_at,updated_at) VALUES('Range cash',?,?)",(str(now-200),str(now-200)))
c.execute("INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,created_at,updated_at) VALUES(1,'cash','cash','Range cash','cash',16000,?,?,?)",(today.isoformat(),str(now-200),str(now-200)))
meta=json.dumps(dict(flowsChecked=True,cashEventThrough=0,transactionThrough=0,revision='0',attribution=dict(version=1,complete=True,pendingTransactionIds=[],securities=[])))
for n,value in [(2,10000),(1,12000)]:
 c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?,?,?,0,'ready','isolated range fixture',?,?)",((today-datetime.timedelta(days=n)).isoformat(),value,value,str(now-100),meta))
c.commit();c.close()
`,join(fakeHome,'.natives-local/apps/fund/data/fund.db')],{env,encoding:'utf8'});
 assert.equal(seed.status,0,seed.stderr);
 await page.goto(`chrome-extension://${extensionId}/app.html?app=fund`);await page.bringToFront();
 await frame.locator('#home-privacy-btn').waitFor();
 const runtime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
 await frame.locator('.home-history > summary').click();
 await runtime.evaluate(()=>Fund.home.applyDisplayMode('amount',false));
 const dates=await runtime.evaluate(()=>{const today=Fund.home.snapshot().requestedRange.today;const back=n=>new Date(Date.parse(today+'T12:00:00Z')-n*86400000).toISOString().slice(0,10);return {today,yesterday:back(1),first:back(2)};});
 for(const [range,value] of [['all','6000'],['today','4000'],['month',dates.today.endsWith('-01')?'4000':'6000'],['year',dates.today.endsWith('-01-01')?'4000':'6000']]){
  await frame.locator('#hc-return [data-range='+range+']').click();
  await runtime.waitForFunction(range=>Fund.home.snapshot().requestedRange.key===range,range);
  const snapshot=await runtime.evaluate(()=>Fund.home.snapshot());
  assert.equal(snapshot.wealth.returns.netReturnMinor,value);assert.equal(snapshot.wealth.returns.status,'reference');
  assert.equal(snapshot.portfolio.history.to,dates.today);assert.equal(snapshot.portfolio.history.points.at(-1).quality,'reference');
  report.steps.push({range,returns:snapshot.wealth.returns,history:{from:snapshot.portfolio.history.from,to:snapshot.portfolio.history.to}});
 }
 const shots=resolve(import.meta.dirname,'../../.tmp/home-range/native-shots');mkdirSync(shots,{recursive:true});
 for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [1280,320]){
  await page.setViewportSize({width,height:900});
  await runtime.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));},{theme,language});
  const trigger=frame.locator('#hc-return [data-range=custom]');await trigger.click();
  const pane=frame.locator('#home-custom-range');assert.equal(await pane.evaluate(el=>el.matches(':popover-open')),true);
  const month=dates.yesterday.slice(0,7);
  await pane.locator('[data-range-year]').fill(month.slice(0,4));await pane.locator('[data-range-year]').press('Tab');
  await pane.locator('[data-range-month]').selectOption(String(Number(month.slice(5))));
  const date=pane.locator('[data-date="'+dates.yesterday+'"]:not(.range-outside)').first();
  await date.focus();await date.press('Enter');
  await page.screenshot({path:join(shots,`${theme}-${language}-${width}.png`)});
  const bounds=await pane.evaluate(el=>{const r=el.getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,width:innerWidth,height:innerHeight,clientWidth:document.documentElement.clientWidth};});report.steps.push({theme,language,width,bounds});
  assert.ok(bounds.left>=0&&bounds.right<=bounds.width+1&&bounds.top>=0&&bounds.bottom<=bounds.height+1,JSON.stringify(bounds));
  await date.press('Enter');await runtime.waitForFunction(d=>Fund.home.snapshot().requestedRange.key==='custom'&&Fund.home.snapshot().requestedRange.to===d,dates.yesterday);
  const snapshot=await runtime.evaluate(()=>Fund.home.snapshot());assert.equal(snapshot.wealth.returns.netReturnMinor,'2000');assert.equal(snapshot.wealth.returns.status,'ready');
  assert.equal(await trigger.evaluate(el=>document.activeElement===el),true,'two-click selection restores trigger');
  await trigger.click();await pane.press('Escape');assert.equal(await pane.evaluate(el=>el.matches(':popover-open')),false);
 }
 const invalid=await runtime.evaluate(async()=>{try{await Fund.api('GET','/api/home/snapshot?range=custom&from=2026-02-30&to=2026-03-01');return 200;}catch(e){return e.status;}});assert.equal(invalid,400);
 const count=spawnSync('python3',['-c','import sqlite3,sys;print(sqlite3.connect(sys.argv[1]).execute("SELECT COUNT(*) FROM value_snapshots").fetchone()[0])',join(fakeHome,'.natives-local/apps/fund/data/fund.db')],{env,encoding:'utf8'});
 assert.equal(count.stdout.trim(),'2','range changes and live projections never save valuation snapshots');
 report.steps.push({name:'real Native ranges; read-only current reference, historical custom actual net return, Archive/Volt, zh/en, narrow/keyboard/Escape/focus, invalid dates',shots});
}
