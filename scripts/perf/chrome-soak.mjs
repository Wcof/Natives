// Real isolated Chrome soak. Called by the existing Native browser harness.
import assert from 'node:assert/strict';
import { readFileSync, appendFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

export function installResourceProbe() {
  const timeouts = new Set(), intervals = new Set(), ports = new Set();
  const timeout = globalThis.setTimeout.bind(globalThis), clear = globalThis.clearTimeout.bind(globalThis);
  const interval = globalThis.setInterval.bind(globalThis), clearInterval = globalThis.clearInterval.bind(globalThis);
  globalThis.setTimeout = (fn, ms, ...args) => {
    let id;
    id = timeout(typeof fn === 'function' ? (...values) => { timeouts.delete(id); return fn(...values); } : fn, ms, ...args);
    timeouts.add(id); return id;
  };
  globalThis.clearTimeout = id => { timeouts.delete(id); intervals.delete(id); clear(id); };
  globalThis.setInterval = (fn, ms, ...args) => { const id = interval(fn, ms, ...args); intervals.add(id); return id; };
  globalThis.clearInterval = id => { intervals.delete(id); timeouts.delete(id); clearInterval(id); };
  if (globalThis.chrome?.runtime?.connectNative) {
    const connect = chrome.runtime.connectNative.bind(chrome.runtime);
    chrome.runtime.connectNative = (...args) => {
      const port = connect(...args); ports.add(port);
      const disconnect = port.disconnect.bind(port);
      port.disconnect = () => { ports.delete(port); return disconnect(); };
      port.onDisconnect.addListener(() => ports.delete(port)); return port;
    };
  }
  globalThis.soakResources = () => ({ timeouts: timeouts.size, intervals: intervals.size, nativePorts: ports.size });
}

const cpuMillis = text => {
  const parts = text.split(':').map(Number);
  return parts.reduce((total, part) => total * 60 + part, 0) * 1000;
};

export function processSample(profile, spawnLog) {
  const result = spawnSync('rtk', ['proxy', 'ps', '-axo', 'pid=,ppid=,rss=,time=,lstart=,command='], { encoding: 'utf8', timeout:2000 });
  assert.equal(result.status, 0, 'process sampling failed');
  const rows = result.stdout.split('\n').flatMap(line => {
    const m = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\w{3}\s+\w{3}\s+\d+\s+[\d:]+\s+\d{4})\s+(.*)$/.exec(line);
    return m ? [{ pid:+m[1], parent:+m[2], rssKiB:+m[3], cpuMs:cpuMillis(m[4]), started:m[5], command:m[6] }] : [];
  });
  const ownedPids = new Set(rows.filter(row => row.command.includes(dirname(profile) + '/')).map(row => row.pid));
  const hosts = new Map(spawnLog.filter(item => ownedPids.has(item.pid)).map(item => [item.pid, item.host]));
  const selected = new Set(rows.filter(row => row.command.includes(`--user-data-dir=${profile}`) || hosts.has(row.pid)).map(row => row.pid));
  for (let changed = true; changed;) {
    changed = false;
    for (const row of rows) if (selected.has(row.parent) && !selected.has(row.pid)) { selected.add(row.pid); changed = true; }
  }
  assert.ok(selected.size > 0, 'isolated Chrome process tree not found');
  return rows.filter(row => selected.has(row.pid)).map(({command, ...row}) => ({
    ...row, role: hosts.get(row.pid) || (command.includes('--type=renderer') ? 'renderer' : 'chrome'),
    identity: `${row.pid}:${row.started}`,
  }));
}

// An iframe may be removed between enumerating frames and reading its context.
export async function frameSample(frame) {
  const url = frame.url();
  let timer;
  try {
    const values = await Promise.race([
      frame.evaluate(() => ({visibility:document.visibilityState,resources:globalThis.soakResources?.() ?? null})),
      new Promise((_, reject) => {timer=setTimeout(() => reject(Error('frame sampling timeout')),1500);}),
    ]);
    return {url,...values};
  } catch (error) {
    if (frame.isDetached() || /Execution context was destroyed|Cannot find context with specified id|Frame was detached/.test(error.message)) {
      return {url,visibility:null,resources:null,unavailable:'navigation'};
    }
    throw error;
  } finally { clearTimeout(timer); }
}

export async function runChromeSoak({ context, space, profile, out, reportOut, report, extensionId, smoke = false }) {
  const url = path => `chrome-extension://${extensionId}/${path}`;
  // The isolated Host owns all widget records, including seven real AI widgets and five clocks.
  await space.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const { widgetPlugins } = await import('./plugins/widgets/index.js');
    const client = createNativeClient({host:'com.natives.file_manager',handshake:{method:'version',params:{extensionVersion:chrome.runtime.getManifest().version}},writeMethods:['workspace_widget_upsert']});
    try {
      const session = await client.call('workspace_session');
      const id = session.workspaces.find(w => w.name === 'Isolated investment acceptance').id;
      let snapshot = await client.call('workspace_snapshot', {workspaceId:id});
      const keys = ['aiCost','aiTokens','aiSessions','aiRequests','aiLimits','aiAttention','aiSavings'].map(key => 'widget/'+key).concat(Array(5).fill('widget/time'));
      for (const key of keys) {
        const definition = widgetPlugins[key];
        if (!definition) throw Error(`missing real widget ${key}`);
        snapshot = await client.call('workspace_widget_upsert', {workspaceId:id,expectedRevision:snapshot.revision,widget:{id:'',key,enabled:true,order:snapshot.widgets.length,configJson:definition.defaultData||{},displayJson:{position:'middleCentre'}}});
      }
    } finally { client.disconnect(); }
  });
  await space.reload();
  await space.locator('.Widget').nth(31).waitFor({timeout:30000});
  await space.evaluate(() => { window.recordSnapshotQueries=false; window.snapshotQueries.length=0; });
  const files = await context.newPage(), apps = await context.newPage(), fund = await context.newPage();
  await files.goto(url('files.html')); await files.locator('#entries').waitFor();
  await apps.goto(url('apps.html')); await apps.locator('.app-card[data-app-id="fund"] .action.primary').waitFor({timeout:30000});
  const pages = [space, files, apps, fund];
  const sessions = new Map(await Promise.all(pages.map(async page => [page, await context.newCDPSession(page)])));
  const samples = [], cycles = [], samplingErrors = [];
  let phase = 'warmup', stopped = false;
  const sample = async () => {
    const entry = { at:new Date().toISOString(), phase, processes:processSample(profile, readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').filter(Boolean).map(JSON.parse)), pages:[] };
    for (const page of pages) {
      if (page.isClosed()) continue;
      const dom = await sessions.get(page).send('Memory.getDOMCounters');
      const frames = await Promise.all(page.frames().map(frameSample));
      entry.pages.push({url:page.url(),dom,frames});
    }
    samples.push(entry); appendFileSync(join(reportOut,'soak-samples.jsonl'), JSON.stringify(entry)+'\n');
  };
  const sampling = (async () => {
    let next = performance.now();
    while (!stopped) {
      try { await sample(); } catch (error) { samplingErrors.push({at:new Date().toISOString(),message:error.message}); }
      next += 9_000;
      await delay(Math.max(0,next-performance.now()));
    }
  })();
  const cycle = async () => {
    const start = performance.now();
    await space.bringToFront();
    await space.locator('#settings-entry').click();
    await space.locator('#settings-menu .menu-row').nth(1).click();
    await space.locator('.model-settings-dialog[open]').waitFor();
    await space.keyboard.press('Escape');
    await space.locator('.model-settings-dialog').waitFor({state:'hidden'});
    await files.bringToFront(); await files.locator('#refresh').click();
    await apps.bringToFront();
    await fund.bringToFront();
    await fund.goto(url('app.html?app=fund'));
    const frame = fund.frameLocator('iframe');
    await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
    await frame.locator('#tab-positions').click(); await frame.locator('#tab-home').click();
    let runtime = readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').map(JSON.parse).filter(item=>item.host==='com.natives.local.app_runtime').at(-1).pid;
    // Browser visibility lifecycle: hidden while Files is selected, then restored.
    await files.bringToFront();
    await fund.waitForFunction(() => document.visibilityState === 'hidden', null, {timeout:5000});
    let hiddenReclaimMs;
    if (!smoke && cycles.length === 0) {
      const hiddenStart = performance.now();
      await delay(60_000);
      while (true) {
        try { process.kill(runtime,0); } catch (error) { if (error.code === 'ESRCH') break; throw error; }
        assert.ok(performance.now()-hiddenStart <= 62_000, 'hidden Runtime did not reclaim after 60s idle + 2s EOF');
        await delay(20);
      }
      hiddenReclaimMs = performance.now()-hiddenStart;
      assert.equal(await fund.evaluate(() => globalThis.soakResources().nativePorts), 0, 'hidden App port remains owned');
    } else await delay(100);
    await fund.bringToFront();
    await fund.waitForFunction(() => document.visibilityState === 'visible', null, {timeout:5000});
    await frame.locator('#home-privacy-btn').waitFor({timeout:30000});
    runtime = readFileSync(join(out,'spawns.jsonl'),'utf8').trim().split('\n').map(JSON.parse).filter(item=>item.host==='com.natives.local.app_runtime').at(-1).pid;
    const closeStart = performance.now(); await fund.goto('about:blank');
    while (true) {
      try { process.kill(runtime,0); } catch (error) { if(error.code==='ESRCH') break; throw error; }
      assert.ok(performance.now()-closeStart<=2000, 'Runtime did not exit within EOF budget');
      await delay(20);
    }
    cycles.push({phase,at:new Date().toISOString(),elapsedMs:performance.now()-start,hiddenReclaimMs,eofMs:performance.now()-closeStart});
    await space.bringToFront();
  };
  const started = Date.now();
  try {
    for (const [name,duration] of [['warmup',smoke?10_000:300_000],['run',smoke?10_000:1_800_000]]) {
      phase = name; const end = Date.now()+duration;
      while(Date.now()<end) { await cycle(); await delay(1000); }
      await sample();
      console.log(`Chrome soak ${name} complete: ${cycles.length} cycles, ${samples.length} samples`);
    }
    phase = 'idle'; await space.bringToFront(); await sample();
    await delay(smoke?10_000:60_000); await sample();
  } finally {
    stopped = true; await sampling;
    report.soak = {scope:'real headless Chrome with native tab visibility (CDP noDefaults), isolated ledger; Release App Runtime, development Files Host, Go process-test Model Host with in-memory Secrets',productionReleaseChain:'blocked: production Files Host rejects development trust root',smoke,startedAt:new Date(started).toISOString(),endedAt:new Date().toISOString(),cycles,samplingErrors,sampleCount:samples.length};
    writeFileSync(join(reportOut,'soak-summary.json'),JSON.stringify(report.soak,null,2));
  }
  assert.deepEqual(samplingErrors,[], 'continuous sampling errors');
  const warm = samples.filter(s=>s.phase==='warmup').at(-1), idle = samples.filter(s=>s.phase==='idle');
  const last = idle.at(-1), first = idle[0];
  const baseline = new Map(warm.processes.map(p=>[p.identity,p]));
  const growth = last.processes.filter(p=>p.role==='renderer'&&baseline.has(p.identity)).map(p=>({identity:p.identity,beforeKiB:baseline.get(p.identity).rssKiB,afterKiB:p.rssKiB,growthPercent:(p.rssKiB/baseline.get(p.identity).rssKiB-1)*100}));
  const rendererBeforeKiB = warm.processes.filter(p=>p.role==='renderer').reduce((sum,p)=>sum+p.rssKiB,0);
  const rendererAfterKiB = last.processes.filter(p=>p.role==='renderer').reduce((sum,p)=>sum+p.rssKiB,0);
  const rendererTotalGrowthPercent = (rendererAfterKiB/rendererBeforeKiB-1)*100;
  const idleBase = new Map(first.processes.map(p=>[p.identity,p.cpuMs]));
  const idleCpuPercent = last.processes.reduce((sum,p)=>sum+(idleBase.has(p.identity)?Math.max(0,p.cpuMs-idleBase.get(p.identity)):p.cpuMs),0)/(Date.parse(last.at)-Date.parse(first.at))*100;
  const gaps = samples.slice(1).map((s,i)=>Date.parse(s.at)-Date.parse(samples[i].at));
  Object.assign(report.soak,{growth,rendererBeforeKiB,rendererAfterKiB,rendererTotalGrowthPercent,idleCpuPercent,maxSampleGapMs:Math.max(...gaps),budgets:{rendererGrowthPercent:15,idleTotalCpuPercent:2,filesHostReleaseRss:'separate Release probe required; browser Files Host is development build',appEofMs:2000}});
  writeFileSync(join(reportOut,'soak-summary.json'),JSON.stringify(report.soak,null,2));
  assert.ok(growth.length>0, 'no continuous Renderer identity survived warmup and idle');
  if (!smoke) {
    assert.ok(rendererTotalGrowthPercent<=15, `total Renderer RSS grew ${rendererTotalGrowthPercent}%`);
    assert.ok(growth.every(p=>p.growthPercent<=15),JSON.stringify({growth}));
    assert.ok(idleCpuPercent<=2, `idle total CPU ${idleCpuPercent}% exceeds 2%`);
    assert.ok(Math.max(...gaps)<=10_000,'sampling cadence missed its 10s deadline');
  }
  report.soak.status = smoke ? 'smoke-only' : 'passed';
  writeFileSync(join(reportOut,'soak-summary.json'),JSON.stringify(report.soak,null,2));
  for (const session of sessions.values()) await session.detach();
  for (const page of [files,apps,fund]) await page.close();
}
