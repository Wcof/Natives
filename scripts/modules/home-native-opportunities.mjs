// Real Native opportunity projection, refresh and immutable feedback acceptance.
import assert from 'node:assert/strict';

export async function verifyNativeOpportunities(page, frame, report) {
  if(!await frame.locator('.home-extra-research').evaluate(n=>n.open))await frame.locator('.home-extra-research > summary').click();
  const opportunityRuntime=page.frames().find(f=>f.url().startsWith('http://127.0.0.1:'));
  const opportunityFacts=await opportunityRuntime.evaluate(async()=>{
    await Fund.api('GET','/api/market/kline?symbol=sh600519&period=day&fq=qfq');
    await Fund.actions.refreshHome();
    const snapshot=Fund.home.snapshot();
    return {revision:snapshot.dataRevision,opportunities:snapshot.opportunities};
  });
  assert.equal(opportunityFacts.opportunities.dataRevision,opportunityFacts.revision);
  const opportunityItems=opportunityFacts.opportunities.items;
  assert.equal(opportunityItems.filter(i=>i.securityId==='security:SSE:600519').length,1,'two real ledger accounts merge one candidate');
  assert.equal(opportunityItems.find(i=>i.securityId==='security:SSE:600519').isHolding,true);
  const quotedOpportunity=opportunityItems.find(i=>i.securityId==='security:SSE:600519');
  assert.equal(typeof quotedOpportunity.score,'number','real closed price history must reach scoring path');
  assert.ok(quotedOpportunity.asOf&&quotedOpportunity.source&&quotedOpportunity.evidence);
  assert.notEqual(quotedOpportunity.quality,'insufficient_data');
  for(const item of opportunityItems){
    assert.equal(item.amountRecommendation,null);
    assert.ok(item.conditions && item.invalidation,'conditional plan travels through the real snapshot');
    if(item.constraints.length)assert.ok(['blocked','unavailable'].includes(item.conditions.state));
    if(item.period==='published-nav'){assert.equal(item.conditions.entryRange,null);assert.equal(item.conditions.target,null);}
    assert.equal(item.riskLevel,'unknown','ranking is not a risk assessment');
    if(item.kind==='fund'&&item.evidence)assert.equal(item.evidence.volumeRatio,null);
    if(item.quality==='insufficient_data')assert.equal(item.score,null);
  }
  await frame.locator('#home-opportunity-body [name=holding]').selectOption('all');
  await frame.locator('#home-opportunity-body [name=minScore]').fill('0');
  await frame.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
  // 与 refreshHome 的异步重渲染存在瞬态竞态：等待式断言（阈值不变）。
  await opportunityRuntime.waitForFunction(()=>document.querySelectorAll('[data-opportunity-candidate]').length>=2,undefined,{timeout:20000});
  assert.ok(await frame.locator('[data-opportunity-candidate]').count()>=2,'real stock and fund candidates render after filters');
  assert.ok((await frame.locator('[data-opportunity-candidate="security:SSE:600519"]').textContent()).includes(String(quotedOpportunity.rankScore)),'weighted rank (score x strategy weight) is rendered through production snapshot owner');
  const expectedRank=Math.min(Math.floor(quotedOpportunity.score*Math.max(1000000,...(quotedOpportunity.strategyWeights||[]).map(w=>w.weightPpm))/1000000),100);
  assert.equal(quotedOpportunity.rankScore,expectedRank,'rendered rank equals score x highest matched default weight');
  await frame.locator('[data-opportunity-candidate] summary').first().click();
  assert.equal(await frame.locator('[data-opportunity-candidate] details').first().evaluate(el=>el.open),true);
  report.steps.push({name:'R2 opportunity projection and native filters through real isolated holdings; across-account identity merge; missing score and fund volume gates',items:opportunityItems.map(i=>({securityId:i.securityId,kind:i.kind,isHolding:i.isHolding,quality:i.quality,asOf:i.asOf,score:i.score,action:i.action}))});

  await frame.locator('[data-opportunity-refresh]').click();
  await frame.locator('[data-opportunity-status=complete]').waitFor({timeout:60000});
  const refreshedJob=await opportunityRuntime.evaluate(()=>Fund.api('GET',`/api/opportunities/status?taskId=${document.querySelector('[data-opportunity-task-id]').dataset.opportunityTaskId}`));
  assert.equal(refreshedJob.status,'complete');assert.ok(refreshedJob.total<=10);
  assert.equal(refreshedJob.items.find(i=>i.securityId==='security:SSE:600519').status,'fetched');
  assert.equal(refreshedJob.items.find(i=>i.securityId==='fund:110022').status,'fetched','real public NAV fetch must be connected');
  await opportunityRuntime.waitForFunction(()=>Fund.home.snapshot().opportunities.items.find(i=>i.securityId==='fund:110022')?.quality!=='insufficient_data');
  const refreshedFund=await opportunityRuntime.evaluate(()=>Fund.home.snapshot().opportunities.items.find(i=>i.securityId==='fund:110022'));
  assert.equal(typeof refreshedFund.score,'number');assert.equal(refreshedFund.evidence.volumeRatio,null);
  assert.ok(refreshedFund.asOf&&refreshedFund.source);
  assert.ok(['nav_review','blocked'].includes(refreshedFund.conditions.state));
  assert.equal(refreshedFund.conditions.entryRange,null);
  assert.equal(refreshedFund.conditions.target,null);
  assert.ok(await frame.locator('[data-opportunity-plan]').count()>0,'conditional plans render through the real UI');
  report.steps.push({name:'explicit opportunity refresh through UI and session-owned Runtime; public closed-stock and 250 NAV source fill scoring without a prerequisite chart visit',job:refreshedJob,fund:{score:refreshedFund.score,asOf:refreshedFund.asOf,source:refreshedFund.source,quality:refreshedFund.quality}});

  const auto=await opportunityRuntime.evaluate(async()=>{
    const before=await Fund.api('GET','/api/opportunities/snapshots?page=1');
    for(const body of [{items:[]},[], 'malformed']){
      const rejected=await Fund.authFetch('/api/home/snapshot/generate?range=all',{method:'POST',headers:{'Content-Type':'application/json'},body:typeof body==='string'?body:JSON.stringify(body)});
      if(rejected.status!==400)throw Error('generation must reject client supplied candidates and malformed payloads');
    }
    const one=await Fund.api('POST','/api/home/snapshot/generate?range=all',{});
    const two=await Fund.api('POST','/api/home/snapshot/generate?range=all',{});
    const after=await Fund.api('GET','/api/opportunities/snapshots?page=1');
    return {before:before.total,after:after.total,first:one.dataRevision,second:two.dataRevision,recorded:two.opportunities.recordedSnapshot};
  });
  assert.equal(auto.first,auto.second);assert.equal(auto.before,auto.after);assert.ok(auto.recorded.snapshotId);
  assert.equal(await frame.locator('[data-opportunity-recorded]').count(),1);
  report.steps.push({name:'automatic generation retains full cohorts, settles current financial revision, and repeated generation makes no duplicate samples',...auto});

  await frame.locator('[data-opportunity-capture]').click();
  await frame.locator('[data-opportunity-record-batch]').waitFor({timeout:30000});
  await opportunityRuntime.waitForFunction(()=>!document.querySelector('[data-opportunity-capture]').disabled);
  const savedCandidates=await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/opportunities/snapshots?page=1'));
  assert.ok(savedCandidates.total>=1,'automatic generation may retain earlier cohorts');
  // 发现池（v23）让显式刷新后的快照可能多于初始投影：断言原身份全部保留并记录发现规模。
  const savedIds=savedCandidates.items[0].items.map(i=>i.securityId);
  assert.ok(opportunityItems.every(i=>savedIds.includes(i.securityId)),'original identities survive discovery growth');
  const discoveryIds=savedCandidates.items[0].items.filter(i=>i.pool==='market').map(i=>i.securityId);
  report.steps.push({name:'bounded market discovery after explicit refresh',discoveryCount:discoveryIds.length,discoveryIds});
  const immutable=structuredClone(savedCandidates.items[0]);
  assert.ok(immutable.items.every(i=>i.conditions&&i.invalidation),'captured cohorts retain original conditional plans');
  await frame.locator('[data-opportunity-record-batch] summary').first().click();
  await frame.locator('[data-feedback-reason]').first().fill('Native isolated feedback');
  await frame.locator('[data-feedback-useful=true]').first().click();
  await opportunityRuntime.waitForFunction(()=>!document.querySelector('[data-opportunity-capture]').disabled);
  const initialFeedback=await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/opportunities/stats'));
  assert.equal(initialFeedback.feedback.total,1);assert.equal(initialFeedback.feedback.usefulRatePpm,1000000);
  await frame.locator('[data-feedback-useful=false]').first().click();
  await opportunityRuntime.waitForFunction(()=>!document.querySelector('[data-opportunity-capture]').disabled);
  const edited=await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/opportunities/snapshots?page=1'));
  assert.equal(edited.items[0].items[0].feedback.version,2);
  assert.equal(edited.items[0].snapshotId,immutable.snapshotId);
  assert.equal(edited.items[0].dataRevision,immutable.dataRevision);
  const conflict=await opportunityRuntime.evaluate(async batch=>{
    try {await Fund.api('POST','/api/opportunities/feedback',{requestId:Fund.home.requestId(),snapshotId:batch.snapshotId,dataRevision:batch.dataRevision,candidateId:batch.items[0].candidateId,expectedVersion:1,useful:true,reason:'stale window'});return false;}
    catch(e){return e.status;}
  },immutable);
  assert.equal(conflict,409);
  const finalFeedback=await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/opportunities/stats'));
  assert.deepEqual(finalFeedback.feedback,{total:1,useful:0,usefulRatePpm:0});
  const versions=await opportunityRuntime.evaluate(batch=>Fund.api('GET',`/api/opportunities/feedback/history?${new URLSearchParams({snapshotId:batch.snapshotId,candidateId:batch.items[0].candidateId,page:'1'})}`),immutable);
  assert.equal(versions.total,2);assert.deepEqual(versions.items.map(f=>f.version),[2,1]);
  await frame.locator('[data-feedback-history]').first().click();
  await frame.locator('dialog[open] [data-feedback-history-content] article').first().waitFor();
  assert.equal(await frame.locator('dialog[open] [data-feedback-history-content] article').count(),2);
  await frame.locator('dialog[open] [name=homeDialogClose]').focus();
  await page.keyboard.press('Escape');
  assert.equal(await frame.locator('[data-feedback-history]').first().evaluate(el=>el===document.activeElement),true);
  report.steps.push({name:'original feedback versions queried and rendered through real Native history dialog; Escape returns to opener',versions:versions.items.map(f=>f.version)});
  report.steps.push({name:'immutable complete candidate capture and versioned feedback through real Native UI; latest-only stats and stale-version conflict',savedCount:immutable.items.length,feedback:finalFeedback.feedback,version:2});

  // R0 报价观察：显式观察更新 → 覆盖统计 → 区间渲染 → 三态隐私 → 重放/冲突。
  await frame.locator('[data-observation-refresh]').first().click();
  await opportunityRuntime.waitForFunction(()=>{const b=document.querySelector('[data-observation-refresh]');return b&&!b.disabled;},{timeout:60000});
  const observed=await opportunityRuntime.evaluate(async()=>{
    const stats=await Fund.api('GET','/api/opportunities/stats');
    const batches=[];
    for(let page=1;;page++){
      const history=await Fund.api('GET',`/api/opportunities/snapshots?page=${page}`);
      batches.push(...history.items);
      if(batches.length>=history.total)break;
    }
    return {stats,candidates:batches.reduce((n,b)=>n+b.items.length,0),batchCount:batches.length};
  });
  assert.equal(observed.stats.observations.metric,'direction-hit-not-useful-rate-not-actual-return');
  for(const horizon of ['3','5','10']){
    const coverage=observed.stats.observations.horizons[horizon];
    assert.equal(coverage.total,observed.candidates,'every saved candidate enters the coverage denominator');
    assert.equal(coverage.mature,0,'a batch generated today cannot have mature windows yet');
    assert.equal(coverage.immature+coverage.missingPrice+coverage.windowLost+coverage.signalUnknown+coverage.unobserved,coverage.total,'status buckets partition the denominator');
    assert.equal(coverage.directions.up.hitRatePpm,null,'no sample must render as non-computable, never 0%');
  }
  const statuses=['Mature','Immature','Missing price','Window lost','Original signal unknown'];
  const lineTexts=await frame.locator('[data-observation-horizon]').allTextContents();
  assert.equal(lineTexts.length,3*immutable.items.length,'three horizon lines render per retained candidate');
  for(const line of lineTexts)assert.ok(statuses.some(s=>line.includes(s)),`observation line renders an explicit status: ${line}`);
  // 本场景批次今日生成：全部候选尚未成熟或明确缺价（港美/无缓存），无起点报价可渲染。
  const hasBaseline=await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/opportunities/snapshots?page=1').then(h=>h.items.some(b=>b.items.some(r=>['3','5','10'].some(k=>r.observation?.horizons[k]?.baseline)))));
  if(hasBaseline)assert.ok(lineTexts.some(l=>l.includes('Price')),'amount mode keeps baseline price visible');
  const coverageLine=await frame.locator('[data-observation-coverage="3"]').textContent();
  assert.ok(coverageLine.includes('Mature 0/'),`coverage line renders maturity counts: ${coverageLine}`);
  // 百分比模式：区间与参考变化保留，价格必须遮盖；隐藏模式整节不渲染。
  await frame.locator('#home-privacy-btn').click();
  await opportunityRuntime.waitForFunction(()=>Fund.home.displayMode()==='percent');
  const percentLines=await frame.locator('[data-observation-horizon]').allTextContents();
  assert.equal(percentLines.length,lineTexts.length);
  for(const line of percentLines)assert.ok(!line.includes('Price'),'percent mode must not attach prices');
  await frame.locator('#home-privacy-btn').click();
  await opportunityRuntime.waitForFunction(()=>Fund.home.displayMode()==='hidden');
  assert.equal(await frame.locator('[data-observation-horizon]').count(),0,'hidden mode renders neither identity nor prices');
  await frame.locator('#home-privacy-btn').click();
  await opportunityRuntime.waitForFunction(()=>Fund.home.displayMode()==='amount');
  // API 契约：载荷仅 requestId+snapshotId；重放同结果、异载荷 409、未知快照 404、GET 不接受。
  const contract=await opportunityRuntime.evaluate(async batch=>{
    const requestId=Fund.home.requestId();
    const first=await Fund.api('POST','/api/opportunities/observations/refresh',{requestId,snapshotId:batch.snapshotId});
    const replay=await Fund.api('POST','/api/opportunities/observations/refresh',{requestId,snapshotId:batch.snapshotId});
    let conflict=null;
    try{await Fund.api('POST','/api/opportunities/observations/refresh',{requestId,snapshotId:'snapshot-forged'});}
    catch(e){conflict=e.status;}
    let unknown=null;
    try{await Fund.api('POST','/api/opportunities/observations/refresh',{requestId:Fund.home.requestId(),snapshotId:'snapshot-unknown'});}
    catch(e){unknown=e.status;}
    let method=null;
    try{await Fund.api('GET','/api/opportunities/observations/refresh');}
    catch(e){method=e.status;}
    return {same:JSON.stringify(first)===JSON.stringify(replay),conflict,unknown,method};
  },immutable);
  assert.equal(contract.same,true,'same-payload replay returns the recorded result');
  assert.equal(contract.conflict,409,'requestId reuse with a different snapshot conflicts');
  assert.equal(contract.unknown,404,'unknown snapshots are not observable');
  assert.equal(contract.method,405,'GET never triggers observation writes');
  report.steps.push({name:'R0 quote observation through real Native UI and API: explicit bounded refresh from existing caches, full coverage denominator, immature null rates, three-mode privacy, replay/conflict/404/405',batchCount:observed.batchCount,candidates:observed.candidates,horizons:observed.stats.observations.horizons,contract});

  // R2 策略配置：目录读取 → 权重变更推进 revision 且投影身份变化 → 恢复默认。
  const strategyFacts=await opportunityRuntime.evaluate(async()=>{
    const list=await Fund.api('GET','/api/strategies');
    const before=await Fund.api('GET','/api/home/snapshot');
    const rebound=list.items.find(s=>s.code==='rebound');
    const first=await Fund.api('POST','/api/strategies/update',{requestId:Fund.home.requestId(),code:'rebound',weightPpm:2000000,expectedRevision:list.revision});
    const afterList=await Fund.api('GET','/api/strategies');
    const after=await Fund.api('GET','/api/home/snapshot');
    const fund=after.opportunities.items.find(i=>i.securityId==='fund:110022');
    const restored=await Fund.api('POST','/api/strategies/update',{requestId:Fund.home.requestId(),code:'rebound',restore:true,expectedRevision:afterList.revision});
    const finalList=await Fund.api('GET','/api/strategies');
    const reboundAfter=finalList.items.find(s=>s.code==='rebound');
    return {
      codes:list.items.map(s=>s.code),
      reboundDefault:rebound.weightPpm,
      revisionAdvanced:first.revision>list.revision,
      snapshotChanged:after.opportunities.snapshotId!==before.opportunities.snapshotId,
      strategyRevisionAdvanced:after.opportunities.strategyRevision>before.opportunities.strategyRevision,
      overriddenAfterUpdate:afterList.items.find(s=>s.code==='rebound').overridden,
      restored:!reboundAfter.overridden&&reboundAfter.weightPpm===rebound.weightPpm,
      fundCarriesWeights:Array.isArray(fund&&fund.strategyWeights),
      illegalRejected:null,
    };
  });
  strategyFacts.illegalRejected=await opportunityRuntime.evaluate(async list=>{
    try{await Fund.api('POST','/api/strategies/update',{requestId:Fund.home.requestId(),code:'rebound',weightPpm:999999999,expectedRevision:list.revision});return false;}
    catch(e){return e.status;}
  },await opportunityRuntime.evaluate(()=>Fund.api('GET','/api/strategies')).then(l=>l));
  assert.ok(strategyFacts.codes.includes('trend_follow')&&strategyFacts.codes.includes('market_scan'),'adapted donor catalog exposes the scan strategies');
  assert.ok(strategyFacts.revisionAdvanced,'config revision advances on update');
  assert.ok(strategyFacts.snapshotChanged,'candidate identity changes with the strategy revision');
  assert.ok(strategyFacts.strategyRevisionAdvanced,'projection strategy revision advances');
  assert.ok(strategyFacts.overriddenAfterUpdate,'override is visible in the catalog');
  assert.ok(strategyFacts.restored,'restore clears the manual override');
  assert.ok(strategyFacts.fundCarriesWeights,'candidates carry matched strategy weights');
  assert.equal(strategyFacts.illegalRejected,400,'out-of-range weight is rejected, never clamped');
  report.steps.push({name:'R2 strategy catalog through real Runtime: weight update advances revision and changes candidate identity, out-of-range rejected, restore returns to defaults',...strategyFacts});

  // R2 收口：要紧事合并、候选定位、排序选择器与市场发现覆盖口径。
  await frame.locator('#home-opportunity-body [name=minScore]').fill('0');
  await frame.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
  const urgentFacts=await opportunityRuntime.evaluate(async()=>{
    const snapshot=Fund.home.snapshot();
    const urgent=snapshot.urgent;
    return {
      hasSection:!!document.querySelector('[data-urgent-section]'),
      rows:urgent?urgent.items.length:0,
      kinds:urgent?[...new Set(urgent.items.map(i=>i.kind))]:[],
      priorityModel:urgent?urgent.priorityModel:null,
      locateButtons:document.querySelectorAll('[data-urgent-locate]').length,
      coverage:snapshot.opportunities.poolCoverage,
      marketOption:!!document.querySelector('#home-opportunity-body [name=pool] option[value=market]'),
      sortOption:!!document.querySelector('#home-opportunity-body [name=sort]'),
    };
  });
  assert.ok(urgentFacts.hasSection,'urgent section renders through real snapshot');
  assert.ok(urgentFacts.priorityModel==='event-1000-holding-500-absolute-change');
  assert.ok(urgentFacts.coverage==='recorded-holdings-watchlist-and-bounded-market','coverage names the bounded discovery pool explicitly');
  assert.ok(urgentFacts.marketOption,'market pool filter option exists');
  assert.ok(urgentFacts.sortOption,'sort selector exists');
  assert.equal(urgentFacts.locateButtons,await frame.locator('[data-urgent-locate]').count());
  if(urgentFacts.rows>0){
    const before=await frame.locator('[data-urgent-miss]').evaluate(el=>el.hidden);
    await frame.locator('[data-urgent-locate]').first().click();
    const flashed=await frame.locator('article.urgent-located').count();
    const missed=await frame.locator('[data-urgent-miss]').evaluate(el=>el.hidden);
    assert.ok(flashed===1||missed===false,'locate either flashes a visible candidate or reveals the filtered-out hint');
    report.steps.push({name:'R2 urgent merge locate interaction',flashed,missWasHiddenBefore:before,missedHidden:missed});
  }
  const firstRankDesc=await frame.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
  await frame.locator('#home-opportunity-body [name=sort]').selectOption('rankAsc');
  await frame.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
  const firstRankAsc=await frame.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
  assert.notEqual(firstRankDesc,firstRankAsc,'sort selector reorders real candidates');
  report.steps.push({name:'R2 urgent merge, candidate locate, sort selector and discovery coverage through real Native chain',...urgentFacts,sortReordered:true});

}
