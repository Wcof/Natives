// Synthetic transport only; never bundled into module production resources.
export const installHomeFixture = () => {
      document.getElementById('view-home').hidden = false;
      window.requests = [];
      window.delayedReplies = [];
      window.homeSignals = [];
      window.heldHomeRequests = [];
      const opportunity=(code,kind,held,score)=>({conditions:{state:score==null?'unavailable':kind==='fund'?'nav_review':'review',referencePrice:100,entryRange:kind==='fund'||score==null?null:[99,101],target:kind==='fund'||score==null?null:118.8},invalidation:{below:score==null?null:88.65,reason:'below_support'},candidateId:code,snapshotId:'synthetic-opp-7',securityId:code,name:code==='600519'?'<img src=x onerror=window.injected=true>':`Private ${code}`,code,kind,market:'CN',pool:held?'holdings':'watchlist',isHolding:held,rankScore:score,riskLevel:'unknown',action:'observe',quality:score==null?'insufficient_data':'ready',asOf:'2026-09-30',source:'synthetic-test',strategyTags:score==null?[]:['trend_follow'],constraints:score==null?['insufficient_history']:[],riskCodes:['liquidity_unverified'],evidence:score==null?null:{maArrangement:1,macdState:1,volumeRatio:kind==='fund'?null:2}});
      const snapshot = {
        snapshotId: 'home-synthetic-7', dataRevision: '7', generatedAt: Math.floor(Date.now()/1000),
        wealth: { quality: 'ready', issues: [], totalA: 12345678, investedI: 10000000, latestValuation: '2026-09-30',
          returns: { status: 'ready', from: '2026-09-01', to: '2026-09-30', netReturnMinor: 12345 },
          cash: { cashMinor: 2345678, reserveState: 'configured', reservedMinor: 100000, reservedByOrdersMinor: 0, availableMinor: 2245678, shortfallMinor: 0,
            accounts: [{ accountId: 1, accountName: 'Private Account', cashMinor: 2345678, reservedMinor: 100000, reservedByOrdersMinor: 0, availableMinor: 2245678, reserveState: 'configured' }] } },
        goals: { status: 'ready', goal: { name: 'Private Goal', targetMinor: 20000000, targetDate: '2027-12-31', scopeKind: 'A' }, progress: { computable: true, ratioTenths: 625, scopeValueMinor: 12500000, gapMinor: 7500000, surplusMinor: 0, remainingInvestments: 15, requiredMonthlyMinor: 500000 } },
        allocation: { configured: true, computable: true, total_minor: 12245678, classes: [{ risk_class: 'equity', target_tenths: 500, actual_tenths: 600, delta_tenths: 100, amount_minor: 7347407, status: 'above' }] },
        alerts: { openCount: 1, lastCheck: '2026-09-30' },
        opportunities:{snapshotId:'synthetic-opp-7',dataRevision:'7',inputCount:4,limited:false,items:[opportunity('600519','stock',false,87),opportunity('510300','etf',true,90),opportunity('110022','fund',false,71),opportunity('300750','stock',false,null)]},
        urgent:{items:[
          {kind:'event',securityId:'security:SSE:600519',name:'<img src=x onerror=window.urgentXss=true>',market:'SSE',weight:1502,changePct:2.3,detail:'价格突破 · 12.34',eventId:7,eventStatus:'open',candidateRef:'600519'},
          {kind:'holding',securityId:'security:SSE:600036',name:'Private Holding',market:'SSE',weight:501,changePct:1.0,detail:'constraints:2',eventId:null,eventStatus:null,candidateRef:'600036'},
          {kind:'opportunity',securityId:'fund:110022',name:'Private Fund',market:'CN',weight:5,changePct:5.0,detail:'build_candidate',eventId:null,eventStatus:null,candidateRef:'110022'},
        ],truncated:false,coverage:'same-snapshot-merge-of-events-and-opportunities',priorityModel:'event-1000-holding-500-absolute-change'},
        portfolio: {
          offlineIncome:{count:1,valueMinor:'80000',costMinor:'100000',bookProfitMinor:'29000',netRecoveredMinor:'49000',receivableMinor:'49000',unknownCostCount:0,staleCount:0},
          ledger:{scope:'active-ledger-securities',marketValueMinor:'10000000',holdingProfitMinor:'12345',realizedMinor:'67890',day:{date:new Date(Date.now()+8*3600000).toISOString().slice(0,10),amountMinor:null,missingCount:1,complete:false,holdingCount:1},navs:[],byType:{stock:{holdingCount:1,marketValueMinor:'4000000',holdingProfitMinor:'12345',costMinor:'3987655',realizedMinor:'67890',unknownCostCount:0,missingValueCount:0,day:{date:'2026-10-10',amountMinor:'100',missingCount:0}},fund:{holdingCount:1,marketValueMinor:'6000000',holdingProfitMinor:'0',costMinor:'6000000',realizedMinor:'0',unknownCostCount:0,missingValueCount:0,day:{date:'2026-10-10',amountMinor:null,missingCount:1}}}},
          attribution:{status:'ready',from:'2026-09-27',to:'2026-09-30',totalNetReturnMinor:'987654',securitiesNetContributionMinor:'87000',unallocatedIncomeMinor:'10000',cashFeesMinor:'3000',otherContributionMinor:'893654',itemCount:1,items:[{code:'110022',priceContributionMinor:'90000',feesMinor:'3000',netContributionMinor:'87000'}]},
          securities:{unrealizedMinor:'12345',realizedMinor:'67890',missingValueCount:1,unknownCostCount:1,staleCount:0},
          diagnostics:{coverage:'partial',hhiBps:5200,securities:[{key:'<img src=x onerror=window.injected=true>',weightBps:6000}]},
          dailyReturn:{status:'incomplete',netReturnMinor:null},
          history:{reason:'missing_daily_history',twrPpm:null,points:[
            {date:'2026-09-27',valueMinor:'12345678',quality:'ready'},
            {date:'2026-09-28',valueMinor:'13000000',quality:'ready'},
            {date:'2026-09-30',valueMinor:'15000000',quality:'ready'},
            {date:'2026-10-01',valueMinor:'16000000',quality:'ready'},
          ]},
        },
        // 方案 11 §4：已配置分组投影（A 分母）。percent 模式下占比可见、金额遮蔽。
        groupAllocation: { configured: true, groups: [
          { groupId: 'g1', name: 'Equity Group', ratioTenths: 600, currentTenths: 500, currentMinor: 6172839, gapTenths: -100 },
          { groupId: 'g2', name: 'Cash Group', ratioTenths: 400, currentTenths: 500, currentMinor: 6172839, gapTenths: 100 },
        ], unassignedMinor: 0, invalidRefs: [], denominatorMinor: 12345678, quality: 'complete' },
      };
      const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
      // R08 竞态夹具：slowSnapshot=true 时挂起快照响应，releaseSnapshot() 放行迟到响应。
      const slowSnapshot = () => ({ ...snapshot, groupAllocation: { ...snapshot.groupAllocation, groups: snapshot.groupAllocation.groups.map((g, i) => ({ ...g, currentTenths: i === 0 ? 700 : 300, gapTenths: i === 0 ? 100 : -100 })) } });
      window.fetch = async (url, options) => {
        const path = String(url);
        window.requests.push({ path, body: options?.method === 'POST' || options?.method === 'PUT' ? JSON.parse(options.body || '{}') : null, method: options?.method });
        if (options?.signal) window.homeSignals.push({ path, signal: options.signal });
        if(path.startsWith('/api/opportunities/feedback/history')){
          if(window.failFeedbackHistory)return json({error:'FIN_DB',message:'中文内部错误'},500);
          const result={page:1,pageSize:20,total:window.feedbackVersions?.length||0,items:[...(window.feedbackVersions||[])].reverse()};
          if(window.holdFeedbackHistory)await new Promise(resolve=>{window.releaseFeedbackHistory=resolve;});
          return json(result);
        }
        if(path==='/api/opportunities/capture'){
          window.savedOpportunity={...structuredClone(snapshot.opportunities),generatedAt:'1790740800',items:snapshot.opportunities.items.map(row=>({...row,feedback:{version:0,useful:null,reason:''}}))};
          return json(window.savedOpportunity);
        }
        if(path.startsWith('/api/opportunities/snapshots'))return json({page:1,pageSize:10,total:window.savedOpportunity?1:0,items:window.savedOpportunity?[window.savedOpportunity]:[]});
        if(path==='/api/opportunities/stats'){
          const f=window.savedOpportunity?.items.filter(row=>row.feedback.version>0)||[];
          return json({feedback:{total:f.length,useful:f.filter(row=>row.feedback.useful).length,usefulRatePpm:f.length?f.filter(row=>row.feedback.useful).length*1000000/f.length:null},groups:[]});
        }
        if(path==='/api/opportunities/feedback'){
          const v=JSON.parse(options.body),row=window.savedOpportunity.items.find(r=>r.candidateId===v.candidateId);
          window.feedbackOps??={};if(window.feedbackOps[v.requestId])return json(window.feedbackOps[v.requestId]);
          if(window.feedbackConflict)return json({error:'FIN_CONFLICT'},409);
          row.feedback={version:row.feedback.version+1,useful:v.useful,reason:v.reason};window.feedbackVersions??=[];window.feedbackVersions.push({...structuredClone(row.feedback),createdAt:'1790740800'});window.feedbackOps[v.requestId]=structuredClone(row.feedback);
          if(window.failFeedbackReply)return json({error:'FEEDBACK_REPLY_LOST'},503);
          if(window.holdOpportunityFeedback)await new Promise(resolve=>{window.releaseOpportunityFeedback=resolve;});
          return json(row.feedback);
        }
        if(path==='/api/opportunities/refresh'){
          const taskId=(window.opportunityTask?.taskId||0)+1;
          window.opportunityTask={taskId,requestId:JSON.parse(options.body).requestId,status:'running',completed:0,total:2,scope:JSON.parse(options.body).scope,items:[],poolId:'a'.repeat(64),nextOffset:null};
          if(window.holdOpportunityStart)await new Promise(resolve=>{window.releaseOpportunityStart=resolve;});
          return window.failOpportunityStart?json({error:'OPPORTUNITY_UNAVAILABLE'},503):json({taskId});
        }
        if(path==='/api/opportunities/cancel'){
          const target=JSON.parse(options.body);
          if(target.taskId===window.opportunityTask?.taskId||target.requestId===window.opportunityTask?.requestId)window.opportunityTask.status='cancelled';
          return json({cancelled:true});
        }
        if(path.startsWith('/api/opportunities/status')){
          if(window.failOpportunityStatus)return json({error:'OPPORTUNITY_UNAVAILABLE'},503);
          const result=structuredClone(window.opportunityTask);
          if(window.holdOpportunityStatus)await new Promise(resolve=>{window.releaseOpportunityStatus=resolve;});
          return json(result);
        }
        if(window.holdBenchmark && path.startsWith('/api/portfolio/benchmark')) await new Promise(resolve=>{window.releaseBenchmark=resolve;});
        if (window.holdHomeReads && (path.startsWith('/api/home/snapshot') || path === '/api/market/indices' || path === '/api/check/run' || path === '/api/home/preferences')) {
          // 故意忽略 AbortSignal，复现已返回响应仍迟到的竞态。
          await new Promise(resolve => window.heldHomeRequests.push({ path, resolve, signal: options?.signal }));
        }
        const isGoalConflict = path === '/api/goals/update';
        if (path.startsWith('/api/home/snapshot')) {
          if(window.failRange)return json({error:'FIN_DB'},500);
          if(path.startsWith('/api/home/snapshot/generate')&&window.failGeneration)return json({error:'FIN_DB',message:'中文内部错误'},500);
          // 快照注入只存在于本测试 stub（隔离测试传输）：override 由测试用例
          // 设置，生产页面无任何等价入口（复核 T05）。
          const snap = window.snapshotOverride ? structuredClone(window.snapshotOverride) : (window.slowSnapshot ? await new Promise(resolve => { window.releaseSnapshot = () => resolve(slowSnapshot()); }) : snapshot);
          const query=new URL(path,'http://isolated.test').searchParams;
          const key=query.get('range')||'month',today='2026-10-09';
          snap.requestedRange={key,today,from:key==='custom'?query.get('from'):key==='today'?today:key==='year'?'2026-01-01':key==='month'?'2026-10-01':'',to:key==='custom'?query.get('to'):today};
          if(path.startsWith('/api/home/snapshot/generate'))window.autoOpportunityRecorded=true;
          return json(window.autoOpportunityRecorded?{...snap,opportunities:{...snap.opportunities,recordedSnapshot:{snapshotId:'synthetic-recorded',dataRevision:'6',generatedAt:'1790740800'}}}:snap);
        }
        if(path==='/api/market/indices'&&window.failMarket)return json({error:'APP_UPSTREAM'},503);
        if(path==='/api/home/preferences'&&options?.method==='POST'&&window.bumpPreferenceRevision) snapshot.dataRevision=String(Number(snapshot.dataRevision)+1);
        if(path.startsWith('/api/portfolio/benchmark')&&new URL(path,'http://isolated.test').searchParams.get('dataRevision')!==snapshot.dataRevision) return json({error:'FIN_CONFLICT'},409);
        if (path === '/api/home/group-plan' && options?.method === 'PUT') {
          if (window.planConflict) return json({ error: 'GROUP_REVISION_CONFLICT', message: 'Group plan was changed elsewhere (version conflict)' }, 409);
          const saved = JSON.parse(options.body);
          window.savedPlan = { revision: (window.savedPlan?.revision || 0) + 1, groups: saved.groups.map((g, i) => ({ ...g, groupId: g.groupId || `g${i + 1}` })) };
          snapshot.groupAllocation = { ...snapshot.groupAllocation, groups: saved.groups.map((g, i) => ({ groupId: g.groupId || `g${i + 1}`, name: g.name, description: g.description, ratioTenths: g.ratioTenths, currentTenths: 500, currentMinor: 6172839, gapTenths: g.ratioTenths - 500 })) };
          return json(window.savedPlan);
        }
        const body = isGoalConflict
          ? { error: 'FIN_CONFLICT', message: 'Goal was modified elsewhere (version conflict)' }
          : path === '/api/home/group-plan' ? (window.savedPlan || { groups: [], revision: 0 })
          : path === '/api/home/group-plan/import-preview' ? { available: false }
          : path === '/api/assets' ? { funds: [{ accountId: 1, fundId: 9, name: 'Synthetic Fund', code: '000001', account: 'Private Account', value: 100000, stale: false }], manual: [{ id: 3, name: 'Manual gold', marketValue: 20000, stale: false }] }
          : path === '/api/home/preferences' ? { displayMode: 'hidden', privacyHidden: true, notificationsEnabled: false }
          : path === '/api/check/run' ? { eventCount: 1, actions: [{ resultType: '等待', objectName: 'Private Holding', reason: 'Private 98765', amountCapMinor: 1234500, missing: [], eventId: 3 }] }
          : path === '/api/market/indices' ? { quotes: [{ code: '000001', name: '上证指数', price: 3000, changePct: 0.5, asOf: '2026-09-30 15:00' }] }
          : path.startsWith('/api/portfolio/benchmark') ? { dataRevision:snapshot.dataRevision,symbol:new URL(path,'http://isolated.test').searchParams.get('symbol') || 'sh000300',status:'ready',from:'2026-09-27',to:'2026-09-30',source:'synthetic-test',portfolioPpm:'100000',benchmarkPpm:'50000',excessPpm:'50000',points:[{date:'2026-09-27',portfolioPpm:'0',benchmarkPpm:'0'},{date:'2026-09-30',portfolioPpm:'100000',benchmarkPpm:'50000'}] }
          : path.split('?')[0] === '/api/goals' ? { goals: [{ id: 7, name: 'Synthetic native goal', targetMinor: 20000000, targetDate: '2027-12-31', scopeKind: 'A', assetBindings: '[]', isPrimary: true, archived: false, monthlyPlanMinor: null, version: 1, note: '' }] }
          : path === '/api/rules' ? { rules: [] }
          : path === '/api/sources' ? { sources: [] }
          : path === '/api/information/sources' ? { sources: [{sourceId:'em_stock_ann',enabled:true,lastSuccessAt:null,lastErrorCode:null},{sourceId:'em_stock_news',enabled:true,lastSuccessAt:null,lastErrorCode:null},{sourceId:'em_fund_ann',enabled:true,lastSuccessAt:null,lastErrorCode:null}] }
          : path.split('?')[0] === '/api/theses' ? { theses: [{ id: 1, name: 'User thesis', holdingReason: 'Synthetic only', horizon: 'Long term', verifyMetrics: 'Test', invalidCondition: 'Test', relatedSymbols: '[]' }] }
          : path === '/api/home/evidence' ? (() => { window.lastEvidenceBody = options?.body ? JSON.parse(options.body) : null; return { snapshotId: snapshot.snapshotId, dataRevision: snapshot.dataRevision, disclosedFinance: false, task: window.lastEvidenceBody?.task || 'diagnosis', briefing: window.lastEvidenceBody?.task === 'briefing' ? { market: window.lastEvidenceBody.briefingMarket, session: window.lastEvidenceBody.briefingSession } : null, evidence: [{ id: 'E1', quality: 'ready' }] }; })()
          : path === '/api/finance/cash' ? snapshot.wealth.cash
          : path === '/api/finance/events?limit=100' ? { events: [] }
          : path === '/api/finance/reservations' ? { reservations: [] }
          : path === '/api/finance/trades' ? { trades: [{ id: 9, account: 'Private Account', symbol: 'security:SSE:600519', type: 'BUY', deltaMinor: -10000, status: 'unreconciled' }] }
          : path === '/api/home/notifications/claim' ? { eventIds: [] }
          : { ok: true };
        return json(body, isGoalConflict ? 409 : 200);
      };
      window.addEventListener('message', event => {
        if (event.data?.type !== 'natives:ai') return;
        const { op, requestId, generation, challenge } = event.data;
        const result = op === 'options' ? { models: [{ providerId: 'synthetic', modelId: 'fake', name: 'Synthetic model' }] }
          : op === 'status' ? { done: true, text: 'Synthetic AI explanation' } : { started: true };
        const reply = () => window.postMessage({ type: 'natives:reply', requestId, generation, challenge, result }, '*');
        if (op === 'status' && window.slowStatus) window.delayedReplies.push(reply);
        else reply();
      });
    };
