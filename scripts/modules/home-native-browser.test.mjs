// Real isolated Chrome → Native Messaging → Runtime → Model Host.
// Reuses the Model Host process test entry with in-memory secrets and a local synthetic provider.
import assert from 'node:assert/strict';
import {dispatchNativeScenario} from './home-native-dispatch.mjs';
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, existsSync, copyFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { homedir, tmpdir } from 'node:os';
import { STABLE_EXTENSION_ID } from '../extension-package.mjs';
import {verifyNativeInformation} from './home-native-information.mjs';
import { verifyNativeOpportunities } from './home-native-opportunities.mjs';
import { verifyBriefing, deleteAllReports } from './home-native-briefing.mjs';
import { verifyR1Accounts } from './home-native-r1-accounts.mjs';
import { verifyNativeCurrency } from './home-native-currency.mjs';
import { verifyNativePositionsMenu } from './positions-menu-native.mjs';
import { navigateInvestment, verifyNativeWatchManage, verifyNativeTrends, verifyNativeInvestmentEmpty, verifyNativeInvestmentSuite, verifyNativeDrawingsWatchlist, verifyNativeModelSettingsEntry, verifyNativeEmbeddedSettings } from './investment-suite-native.mjs';
import { verifyFinancialSettings, verifySpaceFinancialSettings, verifyNativeGroupPlan } from './home-native-settings.mjs';
import { seedSpaceWidgetLedger, verifySpaceHostWidgets } from './space-host-widgets.mjs';
import { prepareNativeCandidate, launchSoakBrowser, installNativeTimings } from './home-native-candidate.mjs';
import { installResourceProbe, runChromeSoak } from '../perf/chrome-soak.mjs';
const soak = process.argv.includes('--mode-soak') || process.argv.includes('--soak-smoke');
const tabRetention=process.argv.includes('--tab-retention')||process.argv.includes('--positions-loading');
const holdingCards=process.argv.includes('--watchlist-groups')||process.argv.includes('--positions-income-edit')||process.argv.includes('--home-range')||process.argv.includes('--home-history')||process.argv.includes('--investment-remediation')||process.argv.includes('--inline-ai')||process.argv.includes('--stock-details');
const root = resolve(import.meta.dirname, '../..');
const base = join(root, '.tmp/invest-home-completion');
mkdirSync(base, { recursive: true });
// macOS 拒绝浏览器启动的 Node 读取 Downloads 内测试脚本（EPERM）。
// 仅此单进程隔离夹具放系统临时目录；报告回存项目 .tmp，不改 TCC 或真实注册。
const out = mkdtempSync(join(tmpdir(), 'natives-home-native-'));
const reportOut = mkdtempSync(join(base, 'native-'));
const fakeHome = join(out, 'home');
const config = join(out, 'model-config');
const realHome = homedir();
const env = { ...process.env, HOME: fakeHome, NATIVES_DEV_PRODUCT_SOURCE: join(fakeHome, '.natives-local/product-source'), NATIVES_MODEL_HOST_CONFIG_DIR: config, NATIVES_MODEL_HOST_PROCESS_TEST: '1', RUSTUP_HOME: join(realHome, '.rustup'), CARGO_HOME: join(realHome, '.cargo') };
const isolatedPaths = Object.fromEntries(['HOME', 'NATIVES_DEV_PRODUCT_SOURCE', 'NATIVES_MODEL_HOST_CONFIG_DIR', 'NATIVES_MODEL_HOST_PROCESS_TEST'].map(key => [key, env[key]]));
delete env.NATIVES_APPS_ROOT; delete env.NATIVES_HOME; delete env.CARGO_TARGET_DIR;
if (soak) env.NATIVES_APPS_ROOT = join(fakeHome, '.natives-local/apps');
if (soak) isolatedPaths.NATIVES_APPS_ROOT = env.NATIVES_APPS_ROOT;
mkdirSync(fakeHome, { recursive: true }); mkdirSync(config, { recursive: true });
for (const folder of ['Desktop', 'Documents', 'Downloads']) mkdirSync(join(fakeHome, folder), {recursive:true});
const report = { scope: process.argv.includes('--space-host-widgets') ? 'space-host-widgets' : 'investment-home', ledger: 'isolated-real', nativeMessaging: 'real', syntheticProvider: true, testDirectory: out, steps: [], providerRequests: [], errors: [] };
const pushStep=report.steps.push.bind(report.steps);
report.steps.push=(...items)=>{for(const item of items)console.log('Native check: '+item.name);return pushStep(...items);};
const upstream = createServer(async (req, res) => {
  if (req.url.endsWith('/models')) { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify({ data: [{ id: 'process-model', object: 'model' }] })); return; }
  let body = ''; for await (const chunk of req) body += chunk;
  try {
    assert.equal(req.headers.authorization, 'Bearer process-upstream-key');
    const payload = JSON.parse(body);
    assert.equal(payload.model, 'process-model');
    assert.ok(payload.messages.some(m => m.content.includes('snapshotId')));
    const entry={ model: payload.model, chars: body.length };
    if(process.argv.includes('--inline-ai')){
      const packet=JSON.parse(payload.messages.find(message=>message.content.trim().startsWith('{')&&message.content.includes('snapshotId')).content);
      entry.disclosedFinance=packet.disclosedFinance;
      entry.informationIds=packet.evidence.filter(piece=>piece.kind==='information').map(piece=>piece.id);
      entry.syntheticAccountIncluded=body.includes('Synthetic AI account');
    }
    report.providerRequests.push(entry);
    res.setHeader('Content-Type', 'application/json');
    const content='Synthetic full-chain explanation [E1]. No real account data.';
    res.end(JSON.stringify({ id: 'synthetic-completion', object: 'chat.completion', model: 'process-model', choices: [{ index: 0, message: { role: 'assistant', content: process.argv.includes('--inline-ai')?Array(60).fill(content).join('\n'):content }, finish_reason: 'stop' }], usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 } }));
  } catch (e) { report.errors.push(e.message); res.statusCode = 500; res.end('fixture failure'); }
});
await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
const chrome = process.env.NATIVES_TEST_CHROME || join(realHome, '.cache/puppeteer/chrome/mac_arm-148.0.7778.97/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
let context, chromeChild;
try {
  assert.ok(existsSync(chrome), 'Chrome for Testing is required');
  const { profile, ext } = prepareNativeCandidate({ root, out, fakeHome, config, reportOut, env, isolatedPaths, soak, providerPort: upstream.address().port });
  console.log('Launching isolated Chrome extension…');
  if (soak || tabRetention || holdingCards) ({context,child:chromeChild} = await launchSoakBrowser({chromium,chrome,profile,ext,env,reportOut,report}));
  else context = await chromium.launchPersistentContext(profile, { executablePath: chrome, headless: true, env, ignoreDefaultArgs: ['--disable-extensions'], args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`, '--disable-features=DisableLoadExtensionCommandLineSwitch', '--use-mock-keychain', '--password-store=basic', '--no-first-run'] });
  if (soak) await context.addInitScript(installResourceProbe);
  // Chrome opens an implicit new-tab/Space before the test. It is not a tested
  // second surface; close it before initializing the isolated stores. Explicit
  // multi-surface and EOF checks remain below.
  for (const initial of context.pages()) await initial.close();
  const page = await context.newPage();
  if (soak || tabRetention || holdingCards) await page.bringToFront();
  page.on('pageerror', e => report.errors.push(e.stack || e.message));
  page.on('requestfailed', request => {
    if(request.url().startsWith('http://127.0.0.1:')) (report.failedRequests ||= []).push({url:request.url(),method:request.method(),failure:request.failure()});
  });
  page.on('response', response => {
    if(response.url().startsWith('http://127.0.0.1:') && response.status() >= 400) (report.failedResponses ||= []).push({url:response.url(),status:response.status()});
  });
  await page.addInitScript(installNativeTimings);
  nativeChecks: {
  const start = performance.now();
  await page.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`);
  if(holdingCards||tabRetention) {await page.bringToFront();await page.waitForFunction(()=>document.visibilityState==='visible',null,{timeout:5000});}
  const frame = page.frameLocator('iframe');
  await frame.locator('#home-privacy-btn').waitFor({ timeout: 30000 }).catch(async e => { report.page = { url: page.url(), text: await page.locator('body').innerText() };
 throw e; });
  if(await dispatchNativeScenario({page,frame,context,report,fakeHome,out,env,extensionId:STABLE_EXTENSION_ID})) {assert.deepEqual(report.errors,[]);break nativeChecks;}
  if (soak) {
    // Chrome 的初始新标签页会被扩展重定向到 Space；关闭夹具初始页后再激活被测页。
    for (const initial of context.pages()) if (initial !== page) await initial.close();
    await page.bringToFront();
    await page.waitForFunction(() => document.visibilityState === 'visible', null, {timeout:5000});
  }
  // R2 overview is visible by default; empty recorded data stays unknown.
  await frame.locator('#hc-total-value').getByText('待核实').waitFor({ state: 'attached' });
  report.steps.push({ name: 'real default home', coldOpenMs: performance.now() - start });
  assert.equal(await frame.locator('.home-overview-region').isVisible(),true);
  assert.equal(await frame.locator('.home-opportunities-region').isVisible(),true);
  assert.equal(await frame.locator('#home-history-chart svg').count(),0);
  const overview=await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(async()=>{
    const s=await Fund.api('GET','/api/home/snapshot');
    return {basis:s.portfolio.basis,history:s.portfolio.history,revision:s.dataRevision,snapshotId:s.snapshotId};
  });
  assert.equal(overview.basis,'recorded-ledger-valuations');
  assert.equal(overview.history.source,'value_snapshots');
  assert.equal(overview.history.points.length,0);
  assert.equal(overview.history.twrPpm,null);
  assert.ok(overview.snapshotId.includes(overview.revision));
  report.steps.push({name:'R2 overview + opportunity regions through real Runtime; empty true history has no fabricated curve or return'});
  assert.equal(await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.formatMinorExact('9223372036854775807',false)), '92233720368547758.07');

  if (process.argv.includes('--mode-regression')) {
    report.scope='mode-regression';
    await verifyNativeInvestmentEmpty(page,frame,report);
    await verifyNativeDrawingsWatchlist(page,frame,report);
    await verifyNativeModelSettingsEntry(page,frame,report);
    await seedSpaceWidgetLedger(page); await page.close();
  } else if (process.argv.includes('--space-host-widgets') || soak) { await seedSpaceWidgetLedger(page); await page.close(); } else {
  await verifyNativeInvestmentEmpty(page,frame,report);
  await verifyNativeDrawingsWatchlist(page,frame,report);
  await verifyNativeModelSettingsEntry(page,frame,report);
  // Real historical public announcement: setup only while the isolated Runtime is stopped.
  // Preserve its upstream publication time; no fixture is made artificially current.
  const sampleResponse=await fetch('https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size=1&page_index=1&ann_type=A&stock_list=600519&f_node=0&s_node=0',{signal:AbortSignal.timeout(10000)});
  assert.equal(sampleResponse.status,200);
  const sample=(await sampleResponse.json()).data.list[0];
  assert.ok(sample.art_code&&sample.notice_date&&sample.title);
  const sampleFile=join(out,'public-announcement.json');writeFileSync(sampleFile,JSON.stringify(sample));
  const initialPid=JSON.parse(readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').filter(line=>JSON.parse(line).host==='com.natives.local.app_runtime').at(-1)).pid;
  await page.goto('about:blank');
  const stopUntil=performance.now()+2000;
  while(performance.now()<stopUntil){try{process.kill(initialPid,0);}catch{break;}await new Promise(resolve=>setTimeout(resolve,20));}
  assert.throws(()=>process.kill(initialPid,0),'fixture setup requires stopped Runtime');
  const seed=spawnSync('python3',['-c',`
import sys,json,sqlite3,datetime
sample=json.load(open(sys.argv[2]));date=datetime.datetime.fromisoformat(sample['notice_date'])-datetime.timedelta(hours=8)
stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
c=sqlite3.connect(sys.argv[1])
cur=c.execute("INSERT INTO information_items(source_id,external_id,title,url,published_at,content_text,completeness,time_precision,retrieved_at,content_hash,event_type,importance,created_at,updated_at) VALUES('em_stock_ann',?,?,?,?,?,'excerpt','day',?,'isolated-public-sample','notice',0,?,?)",(sample['art_code'],sample['title'],'https://data.eastmoney.com/notices/detail/600519/'+sample['art_code']+'.html',date.strftime('%Y-%m-%dT%H:%M:%SZ'),sample['title'],stamp,stamp,stamp))
c.execute("INSERT INTO information_relations(item_id,instrument_ref,relation,reason,created_at) VALUES(?,'security:CN:stock:600519','direct','public historical sample',?)",(cur.lastrowid,stamp))
# 合成历史估值存入隔离真实账本；指数由 Runtime 真实公开接口获取。
today=datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).date()
meta=json.dumps(dict(flowsChecked=True,cashEventThrough=0,transactionThrough=0,revision='0',attribution=dict(version=1,complete=True,pendingTransactionIds=[],securities=[])))
for days_ago in range(60,0,-1):
    valued=(today-datetime.timedelta(days=days_ago)).isoformat()
    c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,note,created_at,breakdown_json) VALUES(?,10000,10000,0,'ready','isolated synthetic history','0',?)",(valued,meta))
c.commit();c.close()
`,join(fakeHome,'.natives-local/apps/fund/data/fund.db'),sampleFile],{env,encoding:'utf8'});
  assert.equal(seed.status,0,seed.stderr);
  report.publicSample={externalId:sample.art_code,publishedAt:sample.notice_date,setup:'isolated historical public row with title-only cached excerpt and unknown body time; publication unchanged; body fetched by Runtime'};
  await page.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`);
  await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
  report.nativeTimings = await page.evaluate(() => window.nativeTimings);
  assert.equal(await frame.locator('#view-home').isVisible(), true);
  assert.equal(await frame.locator('#home-privacy-btn').textContent(), '显示金额');
  // R08/方案 11 §5：新用户默认 hidden；切 amount 持久化后，新实例（独立 runtime）保持。
  const persistPage = await context.newPage();
  await persistPage.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`, { waitUntil: 'commit' });
  const persistFrame = persistPage.frameLocator('iframe');
  await persistFrame.locator('#home-privacy-btn').waitFor({ timeout: 30000 });
  assert.equal(await persistFrame.locator('#home-privacy-btn').textContent(), '显示金额', 'second instance must read the same default (hidden)');
  await persistPage.close();
  await page.reload();
  await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
  await frame.locator('#home-privacy-btn').click();
  if(!await frame.locator('.home-history').evaluate(n=>n.open))await frame.locator('.home-history > summary').click();
  await frame.locator('#home-benchmark-chart svg').waitFor({timeout:30000}).catch(async e=>{
    report.benchmarkFailure = await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(async()=>{
      const state={benchmark:Fund.home.benchmark(),snapshot:Fund.home.snapshot()};
      let response;try{response=await Fund.api('GET','/api/portfolio/benchmark?dataRevision='+state.snapshot.dataRevision);}catch(e){response={status:e.status,code:e.code,message:e.message};}
      return {benchmark:state.benchmark,revision:state.snapshot.dataRevision,response};
    });throw e;
  });
  const benchmark = await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.home.benchmark());
  assert.equal(await frame.locator('#home-benchmark-chart polyline').count(),2,'flat zero-return line has zero-height bounds but must remain in the visible SVG');
  assert.equal(benchmark.status,'ready');
  assert.equal(benchmark.source,'proxy.finance.qq.com');
  assert.equal(benchmark.basis,'CNY-price-index');
  assert.equal(benchmark.portfolioPpm,'0');
  assert.ok(benchmark.points.length>=2);
  assert.ok(benchmark.to < new Date(Date.now()+8*3600000).toISOString().slice(0,10),'no intraday candle treated as closed');
  report.steps.push({name:'real benchmark API and Chrome projection with isolated synthetic ledger history; shared closed dates, price-index distinction and no invented weekend points',benchmark});
  await frame.locator('#home-attribution-body summary').click();
  const attribution=await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.home.snapshot().portfolio.attribution);
  assert.equal(attribution.status,'ready');
  assert.equal(attribution.totalNetReturnMinor,'0');
  assert.equal(attribution.otherContributionMinor,'0');
  assert.equal(attribution.scope,'ledger-securities-and-explicit-residual');
  report.steps.push({name:'attribution production projection through real Native isolated ledger with recorded component metadata; genuine zero contributions preserved',attribution});
  await page.waitForTimeout(300);
  const checkPage = await context.newPage();
  await checkPage.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`, { waitUntil: 'commit' });
  const checkFrame = checkPage.frameLocator('iframe');
  await checkFrame.locator('#home-privacy-btn').waitFor({ timeout: 30000 });
  assert.equal(await checkFrame.locator('#home-privacy-btn').textContent(), '只显示比例', 'amount mode must persist across instances');
  await checkPage.close();
  await page.reload();
  await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
  report.steps.push({ name: 'display mode default hidden; amount persists across instances' });
  await frame.locator('#home-goal-manage').click();
  await frame.locator('dialog[open] [data-create]').click();
  await frame.locator('dialog[open] [name=target]').fill('100000');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-edit]').waitFor();
  await frame.locator('dialog[open] [data-close]').click();
  report.steps.push({ name: 'goal CRUD through real Runtime' });
  await frame.locator('.goal-water-content').waitFor();
  const goalVisual = await frame.locator('.goal-water-content').evaluate(node => ({
    value: node.getAttribute('aria-valuenow'), text: node.getAttribute('aria-valuetext'),
    animations: node.getAnimations({ subtree: true }).length,
  }));
  assert.equal(goalVisual.value, null, 'no recorded assets means unavailable goal progress, not zero');
  assert.equal(goalVisual.text, null);
  assert.equal(goalVisual.animations, 0);
  if(!await frame.locator('.home-history').evaluate(n=>n.open))await frame.locator('.home-history > summary').click();
  await frame.locator('#home-cumulative-value').waitFor();
  report.steps.push({ name: 'N05 unavailable goal water level and first-screen cumulative return through real Runtime', goalVisual });
  await frame.locator('#home-ai-explain').click();
  // R09/方案 11 §6：默认模型来自 Model Host 权威 DefaultModelRef；useDefault 预勾选，
  // 走 selection=default + expectedConfigToken，start 返回真实身份字段。
  await frame.locator('dialog[open] [name=useDefault]').waitFor({ state: 'attached' });
  assert.equal(await frame.locator('dialog[open] [name=useDefault]').isChecked(), true, 'default model must be preselected');
  assert.equal(await frame.locator('dialog[open] select[name=model]').inputValue(), 'process-provider::process-model');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-preview] details > summary').click();
  await frame.locator('dialog[open] [data-preview]').getByText('snapshotId', { exact: false }).waitFor();
  await frame.locator('dialog[open] [data-send]').click();
  await frame.locator('dialog[open] [data-result]').getByText('Synthetic full-chain explanation', { exact: false }).waitFor({ timeout: 35000 });
  assert.equal(report.providerRequests.length, 1);
  report.steps.push({ name: 'AI round trip via selection=default through real broker, Native Messaging, Model Host and CLIProxyAPI; synthetic local provider' });
  // 方案 11 §7：用户主动保存报告（resultText + start 返回的真实身份）。
  await frame.locator('dialog[open] [data-save-report]').click();
  await frame.locator('dialog[open] [data-result]').getByText('报告已保存', { exact: false }).waitFor({ timeout: 15000 });
  await verifyBriefing(frame, page, report, assert);
  await verifyFinancialSettings(frame,page,report,assert);
  await frame.locator("#home-ai-explain").click();
  await page.evaluate(() => chrome.storage.local.set({ 'natives-language': 'en', 'natives-theme': 'archive' }));
  await frame.locator('dialog[open] h2').getByText('AI explanation of this assessment').waitFor();
  const runtimeFrame = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
  assert.equal(await runtimeFrame.evaluate(() => document.documentElement.dataset.theme), 'archive');
  report.steps.push({ name: 'real Shell language and theme sync' });
  // ---- 扩展验收（08）：目标编辑/归档/恢复、深链直达、趋势懒加载、快速切换 ----
  // 语言切换会重载页面：等待新 iframe 内首页就绪后再操作。
  await frame.locator('#hc-total-value').waitFor({ state:'attached',timeout: 30000 });
  // 清掉既有脚本语言段可能遗留的 open dialog（拦截指针事件）。
  for (let _i = 0; _i < 2; _i++) {
    const dialogCount = await frame.locator('dialog[open][open]').count();
    if (!dialogCount) break;
    await page.keyboard.press('Escape');
    await page.waitForTimeout(300);
  }
  await frame.locator('#home-goal-manage').click();
  await frame.locator('dialog[open] [data-edit]').click();
  await frame.locator('dialog[open] [name=target]').fill('120000');
  assert.equal(await frame.locator('dialog[open] [name=name]').count(),0);
  assert.equal(await frame.locator('dialog[open] [name=targetDate]').count(),0);
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-archive]').waitFor();
  report.steps.push({ name: 'amount-only goal edit through real Runtime' });
  await frame.locator('dialog[open] [data-archive]').click();
  await frame.locator('dialog[open] [data-yes]').click();
  await frame.locator('dialog[open] details summary').click();
  await frame.locator('dialog[open] [data-restore]').click();
  await frame.locator('dialog[open] [data-edit]').waitFor();
  report.steps.push({ name: 'goal archive + restore through real Runtime' });
  await frame.locator('dialog[open] [data-close]').click();
  // 方案 11 §7：报告列表/查看/删除真实页面路径；不伪造 event_id。
  await frame.locator('#home-reports-manage').click();
  await frame.locator('dialog[open] article').first().waitFor({ timeout: 15000 });
  await frame.locator('dialog[open] details summary').first().click();
  await frame.locator('dialog[open] .decision-original').getByText('Synthetic full-chain explanation', { exact: false }).first().waitFor({ timeout: 15000 });
  await deleteAllReports(frame, page);
  report.steps.push({ name: 'analysis report list/view/delete through real Runtime' });
  await page.keyboard.press('Escape');
  await verifyNativeGroupPlan(frame,page,report,assert);
  // T04/T05: forms write only to the isolated ledger through the real Native chain.
  const today=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
  await frame.locator('#home-cash-manage').click();
  await frame.locator('dialog[open] details summary').first().click();
  const cashForm=frame.locator('dialog[open] form[data-kind=cash]');
  await cashForm.locator('[name=account]').fill('Acceptance cash');
  await cashForm.locator('[name=marketValue]').fill('10000.01');
  await cashForm.locator('[name=confirmed]').check();
  await cashForm.locator('[type=submit]').click();
  await frame.locator('dialog[open] form[data-kind=reserve]').waitFor();
  const reserveForm=frame.locator('dialog[open] form[data-kind=reserve]').first();
  await reserveForm.locator('[name=amount]').fill('0');
  await reserveForm.locator('[type=submit]').click();
  await frame.locator('dialog[open] form[data-kind=reserve] [name=amount][value="0.00"]').first().waitFor();
  await page.keyboard.press('Escape');
  report.steps.push({name:'cash account creation and explicit zero reserve through real forms'});
  const goalRuntime = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
  await goalRuntime.waitForFunction(() => Fund.home.snapshot().goals.progress.computable);
  const knownGoal = await goalRuntime.evaluate(() => {
    const water = document.querySelector('.goal-water-content'), p = Fund.home.snapshot().goals.progress;
    return { value: water.getAttribute('aria-valuenow'), text: water.getAttribute('aria-valuetext'), ratio: p.ratioPpm == null ? p.ratioTenths / 10 : Number(p.ratioPpm) / 10000 };
  });
  assert.equal(Number(knownGoal.value), Math.min(100, knownGoal.ratio));
  assert.equal(knownGoal.text, knownGoal.ratio.toFixed(4).replace(/0+$/, '').replace(/\.$/, '') + '%');
  report.steps.push({ name: 'N05 water level matches real Runtime goal projection after recording cash', knownGoal });
  await frame.locator('#tab-positions').click();
  for(const [kind,code] of [['stock','600519'],['fund','110022']]){
    // 自动 NAV 同步会推进账本版本。等其真实响应后重读持仓，再打开估值草稿；
    // 不把合法的乐观锁 409 当成提交成功，也不绕过净值同步路径。
    const navSync = kind === 'fund' ? page.waitForResponse(r => r.url().endsWith('/api/nav/sync') && r.request().postData()?.includes(code), {timeout:30000}) : null;
    await frame.locator('#holding-add').click();
    await frame.locator('dialog[open] [data-manual]').click();
    await frame.locator('dialog[open] [name=account]').selectOption('Acceptance cash');
    await frame.locator('dialog[open] [name=instrumentKind]').selectOption(kind);
    await frame.locator('dialog[open] [name=fundCode]').fill(code);
    await frame.locator('dialog[open] [name=fundName]').fill('Acceptance '+kind);
    await frame.locator('dialog[open] [name=quantity]').fill(kind==='stock'?'100':'100.1234');
    await frame.locator('dialog[open] [name=totalCost]').fill('1000');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    if (navSync) {
      const response = await navSync;
      report.steps.push({name:'OTC automatic NAV sync settled before opening valuation draft',status:response.status()});
      await page.frames().find(f => f.url().startsWith('http://127.0.0.1:')).evaluate(() => Fund.actions.loadPositions());
    }
    await frame.locator('#pos-tab-'+(kind==='fund'?'otc':kind)).click();
    if(kind==='stock'){
      await frame.locator('.pos-more > summary').first().click();
      await frame.locator('[data-pos-act=valuation]').first().click();
      await frame.locator('dialog[open] [name=price]').fill('0');
      await frame.locator('dialog[open] [name=navCurrency]').selectOption('CNY');
      await frame.locator('dialog[open] [name=riskClass]').selectOption('equity');
      await frame.locator('dialog[open] [type=submit]').click();
      await frame.locator('dialog[open]').waitFor({state:'detached'});
    }
    await frame.locator('#holding-history').click();
    await frame.locator('dialog[open] [data-open]').filter({hasText:code}).click();
    await frame.locator('dialog[open] [data-edit]').click();
    await frame.locator('dialog[open] [name=quantity]').fill(kind==='stock'?'120':'120.1234');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    await frame.locator('#holding-history').click();
    await frame.locator('dialog[open] [data-open]').filter({hasText:code}).click();
    await frame.locator('dialog[open] [data-op=delete]').click();
    await frame.locator('dialog[open] [data-yes]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    await frame.locator('#holding-history').click();
    await frame.locator('dialog[open] [data-open]').filter({hasText:code}).click();
    await frame.locator('dialog[open] [data-op=restore]').click();
    await frame.locator('dialog[open] [data-yes]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    report.steps.push({name:kind+' opening create/edit/archive/restore via real forms; '+(kind==='stock'?'true zero manual valuation':'public NAV owner preserved')});
  }
  await frame.locator('#pos-accounts').click();
  await frame.locator('dialog[open] [data-create]').click();
  await frame.locator('dialog[open] [name=name]').fill('Transfer draft');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open]').waitFor({state:'detached'});
  await frame.locator('#pos-accounts').click();
  await frame.locator('dialog[open] .pos-acct-row').filter({hasText:'Transfer draft'}).locator('[data-rename]').click();
  await frame.locator('dialog[open] [name=name]').fill('Transfer target');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open]').waitFor({state:'detached'});
  await frame.locator('#pos-tab-stock').click();
  if (!await frame.locator('.pos-more').first().evaluate(el=>el.open)) await frame.locator('.pos-more > summary').first().click();
  await frame.locator('[data-pos-act=transfer]').first().click();
  const transferRuntime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
  const transferTarget=await transferRuntime.evaluate(async()=>{
    const data=await Fund.api('GET','/api/accounts/detail');
    return data.items.find(a=>a.name==='Transfer target').id;
  });
  await frame.locator('dialog[open] [name=toId]').selectOption(String(transferTarget));
  await frame.locator('dialog[open] [name=quantity]').fill('20');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] [data-save]:not([disabled])').waitFor();
  const transferCashBefore=await transferRuntime.evaluate(async()=>JSON.stringify(await Fund.api('GET','/api/finance/cash')));
  await frame.locator('dialog[open] [data-save]').click();
  await frame.locator('dialog[open]').waitFor({state:'detached'});
  const transferred=await transferRuntime.evaluate(async()=>await Fund.api('GET','/api/positions/detail'));
  assert.equal(transferred.holdings.find(h=>h.accountId===transferTarget&&h.fundCode==='600519').quantity,'20');
  assert.equal(transferred.holdings.find(h=>h.accountId!==transferTarget&&h.fundCode==='600519').quantity,'100');
  assert.equal(await transferRuntime.evaluate(async()=>JSON.stringify(await Fund.api('GET','/api/finance/cash'))),transferCashBefore);
  report.steps.push({name:'account create and rename; real transfer preview and commit conserve integral stock quantity and cash'});

  await verifyR1Accounts(page, frame, report);
  await verifyNativeCurrency(page, frame, report);
  await verifyNativePositionsMenu(page, frame, report);

  // ---- N04：统一估值批次、首页独立取价、刷新批次变化与四类收益分立 ----
  {
    const runtime = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
    assert.ok(runtime, 'fund runtime frame available for N04 checks');
    const n04 = await runtime.evaluate(async () => {
      const home = await Fund.api('POST', '/api/home/snapshot/generate?range=all');
      const detail = await Fund.api('GET', '/api/positions/detail?refresh=1&kind=all');
      return {
        totalA: home.wealth.totalA, knownA: home.wealth.knownA,
        valuation: home.wealth.valuation, floating: home.wealth.floating,
        cumulative: home.wealth.cumulative, returns: home.wealth.returns,
        detailBatch: detail.valuationBatch,
        rowValues: detail.holdings.map(h => ({ code: h.fundCode, value: h.marketValue, quality: h.valuationQuality })),
      };
    });
    assert.ok(n04.valuation.batch, 'home valuation batch identity present');
    assert.ok(n04.valuation.quoteCount >= 1, 'exchange holdings priced by live quotes at first home open');
    assert.ok(n04.totalA != null, 'totalA complete via unified quotes without visiting positions first');
    assert.ok(n04.detailBatch, 'positions detail exposes its valuation batch');
    const summaryValues = n04.rowValues.filter(r => r.value != null).length;
    assert.ok(summaryValues >= 1, 'row-level unified market values present');
    // 批次含获取时刻（秒）：间隔 >1s 的两次获取即使价格相同也是新批次。
    await new Promise(resolve => setTimeout(resolve, 1100));
    const homeAgain = await runtime.evaluate(async () =>
      (await Fund.api('POST', '/api/home/snapshot/generate?range=all')).wealth.valuation);
    assert.notEqual(homeAgain.batch, n04.valuation.batch, 'refresh yields a new valuation batch without ledger revision bump');
    assert.equal(homeAgain.quoteFailed, false, 'refresh quote fetch succeeded');
    assert.ok(n04.floating && n04.floating.minor != null, 'floating P/L projection present');
    assert.ok(n04.cumulative && n04.cumulative.coverageFrom, 'cumulative return projection with coverage start present');
    assert.ok(n04.returns && (n04.returns.status === 'ready' || n04.returns.status === 'incomplete'), 'period returns stay a separate projection');
    report.steps.push({ name: 'N04 unified valuation: home standalone quotes, batch identity, refresh-new-batch, floating/cumulative/period separated', totalA: n04.totalA, knownA: n04.knownA, batch: n04.valuation.batch });
  }
  await frame.locator('#tab-assets').click();
  for(const kind of ['gold','deposit','wealth','other']){
    await frame.locator('#offline-add').click();
    await frame.locator('dialog[open] [name=productType]').selectOption(kind);
    await frame.locator('dialog[open] [name=name]').fill('Acceptance '+kind);
    await frame.locator('dialog[open] [name=location]').fill('Isolated fixture');
    await frame.locator('dialog[open] [name=value]').fill('2000.01');
    await frame.locator('dialog[open] [name=cost]').fill('1800');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    const assetRow=frame.locator('#offline-cards .offline-asset-card').filter({hasText:'Acceptance '+kind});
    await assetRow.locator('[data-offline=edit]').click();
    await frame.locator('dialog[open] [name=value]').fill('2100.01');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    await assetRow.locator('[data-offline=history]').click();
    await frame.locator('dialog[open] [data-asset-history]').getByText('2100.01',{exact:false}).waitFor();
    await page.keyboard.press('Escape');
    await assetRow.locator('[data-offline=exit]').click();
    await frame.locator('dialog[open] [name=amount]').fill('100');
    await frame.locator('dialog[open] [name=fee]').fill('1');
    await frame.locator('dialog[open] [name=remaining]').fill('2000.01');
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    await assetRow.locator('[data-offline=history]').click();
    await frame.locator('dialog[open] [data-settle]').click();
    await frame.locator('dialog[open] [type=submit]').click();
    await frame.locator('dialog[open]').waitFor({state:'detached'});
    await assetRow.locator('[data-offline=delete]').click();
    await frame.locator('dialog[open] [data-yes]').click();
    await frame.locator('#offline-deleted').check();
    await assetRow.locator('[data-offline=restore]').click();
    await frame.locator('dialog[open] [data-yes]').click();
    await assetRow.locator('[data-offline=edit]').waitFor();
    report.steps.push({name:kind+' create/edit/history/partial exit/deferred settlement/archive/restore via real forms'});
  }
  await frame.locator('#tab-home').click();
  const financeFacts=await page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.api('GET','/api/finance/cash'));
  assert.equal(financeFacts.accounts.find(a=>a.accountName==='Acceptance cash').cashMinor,'1038101');
  report.steps.push({name:'four offline settlements reconcile exact 99.00 each; cash baseline now includes the R1 confirmed OTC buy (15.00 out)'});
  await verifyNativeInvestmentSuite(page,frame,report);

  await verifyNativeInformation(page,frame,report);

  // 深链 view=holdings：新页面加载直接进入持仓视图（ADR-0036）。
  const spawnLog = () => readFileSync(join(out, 'spawns.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
  const runtimePids = () => spawnLog().filter(s => s.host === 'com.natives.local.app_runtime').map(s => s.pid);
  const pidCountBeforeDeep = runtimePids().length;
  const deepPage = await context.newPage();
  deepPage.on('pageerror', e => report.errors.push(`deep: ${e.message}`));
  await deepPage.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund&view=holdings`, { waitUntil: 'commit' });
  const deepFrame = deepPage.frameLocator('iframe');
  await deepFrame.locator('#view-positions:not([hidden])').waitFor({ timeout: 30000 });
  assert.equal(await deepFrame.locator('#view-home').evaluate(el => el.hidden), true, 'deep link must bypass home');
  const deepPid = runtimePids()[pidCountBeforeDeep];
  const deepJob=await deepPage.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.api('POST','/api/opportunities/refresh',{requestId:'deep-eof-refresh',scope:'all',offset:0}));
  assert.ok(deepJob.taskId,'EOF check starts a real session-owned public refresh');
  const deepJobState=await deepPage.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(taskId=>Fund.api('GET',`/api/opportunities/status?taskId=${taskId}`),deepJob.taskId);
  assert.equal(deepJobState.status,'running','EOF is measured with a refresh still in progress');
  const deepClosed = performance.now(); await deepPage.close();
  await new Promise((resolve, reject) => {
    const timer = setInterval(() => { try { process.kill(deepPid, 0); if (performance.now() - deepClosed > 2000) { clearInterval(timer); reject(Error(`deep runtime ${deepPid} did not exit within 2s`)); } } catch { clearInterval(timer); resolve(); } }, 20);
  });
  report.steps.push({ name: 'deep link view=holdings bypasses home; close during opportunity refresh', exitMs: performance.now() - deepClosed, taskId:deepJob.taskId,statusBeforeClose:deepJobState.status });

  await page.reload();
  await frame.locator('#home-privacy-btn').waitFor({timeout:30000});

  await verifyNativeWatchManage(page, report);
  await verifyNativeTrends(page, report);
  const trendsFrame=page.frameLocator('iframe');
  // 快速切换页签：无异常、无未处理拒绝。
  for (const tab of ['tab-watchlist', 'tab-market', 'tab-home', 'tab-trends', 'tab-home']) {
    if(!await trendsFrame.locator(`#${tab}`).isVisible())await trendsFrame.locator('#btn-tools-menu').click();
    await trendsFrame.locator(`#${tab}`).click();
    await page.waitForTimeout(150);
  }
  report.steps.push({ name: 'rapid tab switching without page errors' });
  await verifyNativeOpportunities(page, frame, report);

  // Same-module single-instance contract: exercise a stale editor through a competing
  // authenticated API write in the same real Runtime, without inventing two active owners.
  await frame.locator('#tab-home').click();
  if(await frame.locator('#home-finance-details').getAttribute('open')===null)await frame.locator('#home-finance-details summary').click();
  await frame.locator('#home-group-manage').click();
  await frame.locator('dialog[open] form').waitFor();
  const rt=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
  assert.ok(rt);
  await rt.evaluate(async()=>{
    const p=await Fund.api('GET','/api/home/group-plan');
    await Fund.api('PUT','/api/home/group-plan',{expectedRevision:p.revision,groups:p.groups});
  });
  await frame.locator('dialog[open] [name=name-0]').fill('Draft preserved');
  const rejectReply=page.waitForResponse(r=>r.url().endsWith('/api/home/group-plan')&&r.request().method()==='PUT');
  await frame.locator('dialog[open] [type=submit]').click();
  const rejected=await rejectReply;
  report.conflict={status:rejected.status(),body:await rejected.text(),message:await frame.locator('dialog[open] .home-form-error').textContent()};
  assert.equal(rejected.status(),409);
  await frame.locator('dialog[open] .home-form-error').filter({hasText:/其他窗口|changed|GROUP_PLAN_CONFLICT/}).waitFor({timeout:15000});
  assert.equal(await frame.locator('dialog[open] [name=name-0]').inputValue(),'Draft preserved');
  await page.keyboard.press('Escape');
  report.steps.push({name:'stale group editor → 409, draft preserved (competing authenticated API write)'});
  const mainPid=runtimePids().at(-1);
  assert.ok(Number.isInteger(mainPid),'live owner PID required');
  process.kill(mainPid,0);
  const closed = performance.now(); await page.close();
  const pid = mainPid;
  await new Promise((resolve, reject) => {
    const timer = setInterval(() => { try { process.kill(pid, 0); if (performance.now() - closed > 2000) { clearInterval(timer); reject(Error(`main runtime ${pid} did not exit within 2s`)); } } catch { clearInterval(timer); resolve(); } }, 20);
  });
  report.steps.push({ name: 'real browser close → Runtime EOF', exitMs: performance.now() - closed });
  }
  // Real Space template uses the same isolated ledger and short-lived snapshot port.
  const space=await context.newPage();space.on('pageerror',e=>report.errors.push('space: '+e.message));
  await space.addInitScript(()=>{
    if (!globalThis.chrome?.runtime?.connectNative) return;
    const connect=chrome.runtime.connectNative.bind(chrome.runtime);window.snapshotQueries=[];window.recordSnapshotQueries=true;
    chrome.runtime.connectNative=name=>{const port=connect(name),send=port.postMessage.bind(port);port.postMessage=m=>{if(window.recordSnapshotQueries)window.snapshotQueries.push({name,method:m.method});return send(m);};return port;};
  });
  await space.goto(`chrome-extension://${STABLE_EXTENSION_ID}/space.html`);
  if (soak) await space.bringToFront();
  await space.locator('.ws-select').first().waitFor({timeout:30000});
  await space.locator('#workspace-create').click();await space.locator('#ws-name-input').fill('Isolated investment acceptance');
  await space.locator('#ws-template-select').selectOption('investment');await space.locator('#ws-name-submit').click();
  await space.locator('.InvestmentGoal .inv-card-open').waitFor({timeout:30000});
  await space.waitForFunction(()=>window.snapshotQueries.some(q=>q.method==='app:card_snapshot'));
  assert.equal(await space.locator('.Widget[class*="Investment"]').count(),5);
  assert.equal(await space.evaluate(()=>window.snapshotQueries.filter(q=>q.method==='app:card_snapshot').length),1,'five cards share one snapshot query');
  await space.keyboard.press('Escape');
  await space.locator('#inspector-backdrop').waitFor({state:'hidden'});
  await space.locator('.InvestmentGoal .inv-card-open').click();
  const embedded=space.frameLocator('iframe');await embedded.locator('#view-home:not([hidden])').waitFor({timeout:30000});
  assert.equal(await embedded.locator('#tab-goal').count(),0);
  await space.locator('.ws-select').filter({hasText:'Isolated investment acceptance'}).click();
  await space.locator('.InvestmentGoal .inv-card-open').waitFor({timeout:30000});
  report.steps.push({name:'real Space five-card template → goal deep link → home → return; one shared snapshot query'});
  // Add cards through the real Host API, then verify one fresh query for 20 rendered cards.
  await space.evaluate(async()=>{
    const {createNativeClient}=await import('./native-client.js');
    const client=createNativeClient({host:'com.natives.file_manager',handshake:{method:'version',params:{extensionVersion:chrome.runtime.getManifest().version}},writeMethods:['workspace_widget_upsert']});
    try{
      const session=await client.call('workspace_session');
      const workspace=session.workspaces.find(w=>w.name==='Isolated investment acceptance');
      let snapshot=await client.call('workspace_snapshot',{workspaceId:workspace.id});
      while(snapshot.widgets.length<20){
        snapshot=await client.call('workspace_widget_upsert',{workspaceId:workspace.id,expectedRevision:snapshot.revision,widget:{id:'',key:snapshot.widgets[0].key,order:snapshot.widgets.length,enabled:true,configJson:{},displayJson:{position:'middleCentre'}}});
      }
    }finally{client.disconnect();}
  });
  await space.reload();
  await space.locator('.Widget[class*="Investment"]').nth(19).waitFor({timeout:30000});
  await space.waitForFunction(()=>window.snapshotQueries.some(q=>q.method==='app:card_snapshot'));
  assert.equal(await space.locator('.Widget[class*="Investment"]').count(),20);
  assert.equal(await space.evaluate(()=>window.snapshotQueries.filter(q=>q.method==='app:card_snapshot').length),1,'20 cards share one real Native snapshot query');
  report.steps.push({name:'20 real rendered investment cards share one Native snapshot query after reload'});
  if (soak) {
    report.scope = 'mode-chrome-soak';
    await runChromeSoak({context,space,profile,out,reportOut,report,extensionId:STABLE_EXTENSION_ID,smoke:process.argv.includes('--soak-smoke')});
    await space.close();
  } else {
    await verifyNativeEmbeddedSettings(context, space, report);
    if (process.argv.includes('--mode-regression')) await space.close();
    else await verifySpaceFinancialSettings(space, report, verifySpaceHostWidgets);
  }
  assert.deepEqual(report.errors, []);
  console.log(`${report.scope} real Native Messaging browser chain passed (synthetic provider, in-memory secrets).`);
  }
} catch(error){
  report.failure=error.message;
  report.pages=await Promise.all((context?.pages()||[]).map(async p=>({url:p.url(),nativeFailures:await p.evaluate(()=>window.nativeFailures||[]).catch(()=>[]),text:await p.locator('body').innerText().catch(()=>''),frames:await Promise.all(p.frames().map(async f=>({url:f.url(),text:await f.locator('body').innerText().catch(()=>''),dialog:await f.locator('dialog').innerText().catch(()=>'' )})))})));
  throw error;
} finally {
  await context?.close();
  if (chromeChild && chromeChild.exitCode === null) {
    const exited = new Promise(resolve => chromeChild.once('exit', resolve));
    chromeChild.kill(); await exited;
  }
  upstream.closeAllConnections(); await new Promise(resolve => upstream.close(resolve));
  writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2));
  copyFileSync(join(out, 'report.json'), join(reportOut, 'report.json'));
  console.log(`Report: ${join(reportOut, 'report.json')}`);
}
