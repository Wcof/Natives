import { installHomeFixture } from './home-browser-fixture.mjs';
// 手动执行的真实 Chrome 首页回归，全部数据为合成夹具，不访问用户数据库。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { verifyHomeUsability, verifyHomeEncapsulation } from './home-usability-browser.mjs';
import { verifyHomeDataGaps } from './home-data-gaps-browser.mjs';

const root = resolve(import.meta.dirname, '../../modules/fund/ui/src');
const shots = resolve(import.meta.dirname, '../../.tmp/invest-home-completion/shots');
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  for (const [width, theme] of [1280, 640, 320].flatMap(width => ['volt', 'archive'].map(theme => [width, theme]))) {
    const page = await browser.newPage({ viewport: { width, height: 800 } });
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    const css = ['theme', 'layout', 'panel', 'home', 'investment'].map(name => readFileSync(resolve(name === 'theme' ? resolve(root, '../dist') : root, `styles/${name}.css`), 'utf8')).join('\n');
    await page.setContent(`<style>${css}</style>${readFileSync(resolve(root, 'partials/view-home.html'), 'utf8')}`);
    await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
    await page.evaluate(installHomeFixture);
    for (const file of ['core.js', 'home-i18n.js', 'home-overview.js', 'home-opportunities.js', 'home.js', 'home-range.js', 'home-manage.js', 'home-evidence.js', 'home-report-feedback.js']) await page.addScriptTag({ path: resolve(root, file) });
    await page.evaluate(() => { Fund.state.token = 'synthetic'; Fund.actions.loadHome(); });
    await page.locator('.home-extra-research > summary').click();
    await page.locator('#home-today-body').waitFor();
    await page.waitForFunction(()=>requests.some(request=>request.path==='/api/check/run')&&document.getElementById('home-today-body').getAttribute('aria-busy')==='false');
    if(width===1280&&theme==='archive') {
      const before=await page.evaluate(()=>requests.length);
      await page.evaluate(async()=>{await Fund.actions.refreshHomeValuation();});
      const calls=await page.evaluate(start=>requests.slice(start),before);
      assert.ok(calls.some(r=>r.method==='GET'&&r.path.startsWith('/api/home/snapshot?')),'automatic valuation refresh is read-only');
      assert.ok(!calls.some(r=>r.path.includes('/snapshot/generate')),'automatic ticks do not record opportunity snapshots');
      await page.evaluate(()=>{document.getElementById('home-fund-daily').textContent='sentinel';Fund.home.applyDisplayMode('amount',false);});
      await page.evaluate(async()=>{await Fund.actions.refreshHomeValuation();});
      assert.match(await page.locator('#home-fund-income-note').textContent(),/1/,'daily partial coverage is visible');
      assert.equal(await page.locator('#home-fund-value').textContent(),'60,000.00');
      assert.equal(await page.locator('#home-stock-value').textContent(),'40,000.00');
      assert.equal(await page.locator('#home-offline-profit').textContent(),'290.00');
      assert.equal(await page.locator('.home-income-card').count(),4);
      await page.evaluate(()=>{window.dailyFixture=structuredClone(Fund.home.snapshot());const snapshot=dailyFixture;snapshot.portfolio.ledger.day.date=snapshot.requestedRange.today;snapshot.portfolio.ledger.day.amountMinor='-430';snapshot.portfolio.ledger.day.missingCount=0;snapshot.portfolio.ledger.day.complete=true;Fund.home.renderOverview(snapshot);});
      assert.equal(await page.locator('#home-pending-summary').textContent(),'-4.30','home daily summary reads the same securities daily income, including losses');
      await page.evaluate(()=>{const snapshot=dailyFixture;snapshot.portfolio.ledger.day.amountMinor=null;snapshot.portfolio.ledger.day.complete=false;snapshot.portfolio.ledger.estimatedDay={date:snapshot.portfolio.ledger.day.date,amountMinor:'720',estimatedCount:1,missingCount:0};Fund.home.renderOverview(snapshot);});
      assert.equal(await page.locator('#home-pending-summary').textContent(),'7.20');
      assert.match(await page.locator('#home-daily-summary-note').textContent(),/估算/);
      await page.evaluate(()=>{const snapshot=dailyFixture;snapshot.portfolio.ledger.estimatedDay=null;snapshot.portfolio.ledger.day.amountMinor=null;Fund.home.renderOverview(snapshot);});
      assert.match(await page.locator('#home-pending-summary').textContent(),/待核实/,'missing daily income is not replaced by zero or offline receivables');
      assert.match(await page.locator('#home-fund-income').textContent(),/待核实/);
      await page.evaluate(()=>Fund.home.renderOverview(Fund.home.snapshot()));
      await page.evaluate(()=>Fund.home.applyDisplayMode('hidden',false));
    }

    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `horizontal overflow at ${width}px`);
    assert.equal(await page.locator('#hc-total-value').textContent(), '****');
    assert.equal(await page.evaluate(()=>Fund.formatMinorExact('9223372036854775807',false)),'92233720368547758.07');
    assert.equal(await page.evaluate(() => Fund.home.money(12345678)), '****');
    const cdp = await page.context().newCDPSession(page);
    const ax = await cdp.send('Accessibility.getFullAXTree');
    const names = ax.nodes.map(n => n.name?.value || '').join('\n');
    assert.match(names, /显示金额/);
    assert.doesNotMatch(names, /Private Goal|Private Account|Private Holding|98765|62\.5%/);
    if (width === 1280 || width === 320) await page.screenshot({ path: resolve(shots, `${theme}-${width}-zh.png`), fullPage: true });
    assert.equal(await page.locator('.home-overview-region').count(),1);
    assert.equal(await page.locator('.home-opportunities-region').count(),1);
    assert.equal(await page.locator('#home-history-chart svg').count(),0,'hidden chart must not expose private data');
    assert.equal(await page.locator('#home-benchmark-chart svg').count(),0,'hidden benchmark must not expose private returns');
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),0,'hidden mode removes candidate identity and evidence');
    assert.equal(await page.locator('#home-attribution-body table').count(),0,'hidden attribution must not expose private contributions');
    await page.locator('.home-history > summary').click();
    await verifyHomeUsability(page, { width, theme, shots });
    await verifyHomeEncapsulation(page);
    await verifyHomeDataGaps(page);
    // R2 分配工具暂在折叠区，收益区间位于概览条。
    await page.locator('#home-finance-details summary').click();
    await page.locator('[data-range=custom]').click();
    if(width<560)await page.locator('[data-month-step="-1"]').click();
    await page.locator('[data-date="2026-09-01"]:not(.range-outside)').click();
    await page.locator('[data-date="2026-09-30"]:not(.range-outside)').click();
    await page.waitForFunction(() => window.requests.some(r => r.path.includes('range=custom&from=2026-09-01&to=2026-09-30')));
    await page.waitForFunction(()=>Fund.home.snapshot()?.requestedRange?.key==='custom');
    await page.evaluate(async()=>{window.failRange=true;await Fund.actions.setHomeRange('today');window.failRange=false;});
    assert.equal(await page.locator('.home-history [data-range=custom]').getAttribute('aria-pressed'),'true','failed range retains the last successful range');
    assert.match(await page.locator('#home-range-status').textContent(),/失败|failed/i);
    assert.doesNotMatch(await page.locator('#view-home').textContent(), /Private Account|Private Goal|Private Holding|98765|62\.5%/);
    for (const selector of ['#home-today-rules', '#home-sources-manage', '#home-ai-explain']) {
      await page.locator(selector).click();
      // 体检抽屉 dialog 常驻 DOM（closed）；此处断言无打开的业务模态。
      assert.equal(await page.locator('dialog[open]').count(), 0);
    }
    await page.locator('[data-detail="cash"]').click();
    assert.doesNotMatch(await page.locator('#home-detail-body').textContent(), /Private Account/);
    await page.locator('#home-privacy-btn').click();
    assert.match(await page.locator('#home-detail-body').textContent(), /Private Account/);

    // ---- 方案 11 §5 三态循环：percent 遮金额显占比；hidden 全遮（名称/比例/金额） ----
    await page.locator('#home-privacy-btn').click();
    assert.equal(await page.locator('#hc-total-value').textContent(), '****');
    const groupRow = page.locator('#home-distribution-body details').first();
    await groupRow.waitFor();
    assert.equal(await groupRow.locator('summary .num').textContent(), '**** · 50.0%', 'percent mode must show weight but mask amount');
    assert.match(await groupRow.locator('summary strong').textContent(), /Equity Group/, 'percent mode keeps group names visible');
    assert.doesNotMatch(await page.locator('#home-goal-body').textContent(), /200,000|20\.00万|125,000|12\.50万|75,000|7\.50万/,'percent mode must mask all goal amounts');
    assert.match(await page.locator('#home-goal-progress').textContent(), /62\.5%/);

    await page.locator('#home-privacy-btn').click();
    assert.doesNotMatch(await page.locator('#home-distribution-body').textContent(), /Equity Group|50\.0%|61,728/, 'hidden mode masks names, weights and amounts');
    assert.equal(await groupRow.locator('summary .num').textContent(), '**** · ****');
    await page.locator('#home-privacy-btn').click();
    await page.waitForFunction(() => document.querySelector('#hc-total-value')?.textContent !== '****');
    assert.equal(await page.locator('#home-history-chart polyline').count(),2,'do not connect missing valuation dates');
    await page.locator('#home-benchmark-chart polyline').first().waitFor();
    assert.equal(await page.locator('#home-benchmark-chart polyline').count(),2);
    await page.waitForFunction(() => document.querySelector('#home-benchmark-status')?.textContent.includes('不含分红'));
    assert.match(await page.locator('#home-benchmark-status').textContent(),/不含分红/);
    await page.locator('#home-attribution-body summary').click();
    assert.match(await page.locator('#home-attribution-body').textContent(),/9,876\.54/);
    await page.locator('#home-attribution-body summary').focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('#home-attribution-body details').evaluate(el=>el.open),true);
    assert.equal(await page.locator('#home-attribution-body summary').evaluate(el=>el===document.activeElement),true);
    assert.equal(await page.locator('#home-concentration-list img').count(),0,'security identity must be escaped');
    assert.equal(await page.evaluate(()=>window.injected),undefined);
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),2,'default score and unheld filter');
    assert.equal(await page.locator('#home-opportunity-body img').count(),0,'candidate identity is escaped');
    assert.equal(await page.locator('[data-opportunity-candidate] [data-opportunity-plan]').count(),2);
    assert.ok((await page.locator('[data-opportunity-candidate] [data-opportunity-plan]').first().textContent()).includes('118.8'));
    await page.evaluate(()=>{Fund.home.applyDisplayMode('percent',false);Fund.home.renderOpportunities(Fund.home.snapshot()?.opportunities);});
    assert.ok(!(await page.locator('[data-opportunity-candidate] [data-opportunity-plan]').first().textContent()).includes('118.8'),'percent view does not leak reference prices');
    await page.evaluate(()=>{Fund.home.applyDisplayMode('amount',false);Fund.home.renderOpportunities(Fund.home.snapshot()?.opportunities);});
    await page.locator('.opportunity-filter-more > summary').focus();
    await page.evaluate(()=>Fund.home.renderOpportunities(Fund.home.snapshot()?.opportunities));
    assert.equal(await page.locator('.opportunity-filter-more > summary').evaluate(node=>node===document.activeElement),true,'advanced filter trigger focus survives snapshot redraw');
    await page.locator('.opportunity-filter-more > summary').press('Enter');
    assert.equal(await page.locator('.opportunity-filter-more').evaluate(node=>node.open),true);
    await page.locator('[data-opportunity-candidate] summary').first().click();
    await page.locator('[data-opportunity-candidate] summary').first().focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('[data-opportunity-candidate] details').first().evaluate(el=>el.open),true,'candidate evidence expansion survives redraw');
    assert.equal(await page.locator('[data-opportunity-candidate] summary').first().evaluate(el=>el===document.activeElement),true);
    await page.locator('#home-opportunity-body [name=holding]').selectOption('all');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),3);
    // R2 要紧事：合并区块渲染、身份转义、定位到候选与筛选隐藏提示。
    assert.ok(await page.locator('[data-urgent-section]').count()===1,'urgent section renders');
    assert.ok(await page.locator('[data-urgent-item]').count()===3,'three urgent entries render');
    assert.equal(await page.locator('[data-urgent-item] img').count(),0,'urgent identity is escaped');
    assert.match(await page.locator('[data-urgent-item]').first().textContent(),/触发信号|Triggered signal/,'event kind label localizes');
    assert.equal(await page.locator('[data-urgent-locate]').count(),3,'each entry offers locate');
    await page.locator('[data-urgent-locate]').first().click();
    assert.equal(await page.locator('article.urgent-located').count(),1,'locate flashes the matched candidate');
    await page.locator('#home-opportunity-body [name=kind]').selectOption('fund');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    await page.locator('[data-urgent-locate]').first().click();
    assert.equal(await page.locator('[data-urgent-miss]').evaluate(el=>el.hidden===false),true,'locate miss reveals the hint when filters hide the candidate');
    await page.locator('#home-opportunity-body [data-opportunity-reset]').click();
    // 恢复原流程的 holding=all 状态（reset 会回到默认 unheld）。
    await page.locator('#home-opportunity-body [name=holding]').selectOption('all');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    // R2 排序：rank 升序把低分候选排前；名称序确定稳定。
    const firstRankDesc=await page.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
    await page.locator('#home-opportunity-body [name=sort]').selectOption('rankAsc');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    const firstRankAsc=await page.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
    assert.notEqual(firstRankDesc,firstRankAsc,'sort selector reorders candidates');
    // R2 市场发现筛选选项存在。
    assert.ok(await page.locator('#home-opportunity-body [name=pool] option[value=market]').count()===1,'market pool option exists');
    // R2 列表分页：>20 候选按 20/页渲染，翻页切换切片，过滤收缩后分页消失。
    await page.evaluate(async () => {
      // 快照只读（R-M5）：合成候选注入测试 stub 的传输层（override），经公开
      // refresh 入口重载——生产公开接口无任何发布/替换入口。
      const snap = Fund.home.snapshot();
      window.syntheticRealOpportunities = snap.opportunities;
      const base = snap.opportunities.items.filter(r => r.securityId);
      const extra = Array.from({length: 25}, (_, i) => ({...structuredClone(base[0]), securityId: 'synthetic:extra:' + i, code: String(900000 + i), name: 'Synthetic ' + i, rankScore: 88, strategyTags: [], constraints: [], riskCodes: []}));
      window.snapshotOverride = {...snap, opportunities: {...snap.opportunities, items: [...base, ...extra]}};
      await Fund.actions.refreshHome();
    });
    assert.equal(await page.locator('[data-opportunity-candidate]').count(), 20, 'candidates paginate at 20 per page');
    assert.ok(await page.locator('.opportunity-pager button').count() === 2, 'pager renders for large pools');
    const firstOfPage = await page.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
    await page.locator('[data-candidate-page="1"]').click();
    const firstOfNextPage = await page.locator('[data-opportunity-candidate]').first().getAttribute('data-opportunity-candidate');
    assert.notEqual(firstOfPage, firstOfNextPage, 'next page renders a different slice');
    await page.locator('#home-opportunity-body [name=kind]').selectOption('fund');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    assert.equal(await page.locator('.opportunity-pager').count(), 0, 'pager hides when the filtered set fits one page');
    await page.evaluate(() => {
      // 恢复真实机会集：清除 override 并经公开 refresh 重载真实账本来源快照。
      window.snapshotOverride = null;
      return Fund.actions.refreshHome();
    });
    await page.locator('#home-opportunity-body [name=kind]').selectOption('fund');
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),2,'ETF is in fund filter');
    await page.locator('#home-opportunity-body [name=minScore]').fill('150');
    await page.locator('#home-opportunity-body [name=minScore]').focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('#home-opportunity-body [name=minScore]').inputValue(),'150','unsaved invalid filter draft survives redraw');
    assert.equal(await page.locator('#home-opportunity-body [name=minScore]').evaluate(el=>el===document.activeElement),true);
    await page.locator('#home-opportunity-body form').evaluate(form=>form.requestSubmit());
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),2,'invalid filter does not silently clamp or change applied filter');
    await page.locator('[data-opportunity-reset]').click();
    await page.locator('[data-opportunity-capture]').click();
    await page.locator('[data-opportunity-record-batch]').waitFor();
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-capture]').disabled);
    assert.equal(await page.locator('[data-opportunity-record]').count(),4,'capture includes candidates excluded by current filter');
    await page.locator('[data-opportunity-record-batch] summary').click();
    await page.locator('[data-feedback-reason]').first().fill('<img src=x onerror=window.injected=true>');
    await page.locator('[data-feedback-reason]').first().focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('[data-feedback-reason]').first().inputValue(),'<img src=x onerror=window.injected=true>');
    assert.equal(await page.locator('[data-feedback-reason]').first().evaluate(el=>el===document.activeElement),true);
    assert.equal(await page.locator('[data-opportunity-record-batch]').evaluate(el=>el.open),true);
    await page.locator('[data-feedback-useful=true]').first().click();
    await page.waitForFunction(()=>window.savedOpportunity.items[0].feedback.version===1&&!document.querySelector('[data-opportunity-capture]').disabled);
    await page.locator('[data-feedback-useful=false]').first().click();
    await page.waitForFunction(()=>window.savedOpportunity.items[0].feedback.version===2&&!document.querySelector('[data-opportunity-capture]').disabled);
    assert.equal(await page.locator('.opportunity-records img').count(),0);
    await page.evaluate(()=>window.feedbackConflict=true);
    await page.locator('[data-feedback-useful=true]').first().click();
    await page.locator('.opportunity-records [role=alert]').waitFor();
    assert.equal(await page.locator('[data-feedback-reason]').first().inputValue(),'<img src=x onerror=window.injected=true>','conflict preserves draft');
    await page.evaluate(()=>window.feedbackConflict=false);
    await page.locator('[data-opportunity-records]').click();
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-capture]').disabled);
    assert.equal(await page.locator('[data-opportunity-recorded]').count(),1,'home requests explicit generation rather than silently writing a GET');
    assert.equal(await page.evaluate(()=>window.requests.filter(r=>r.path.startsWith('/api/home/snapshot/generate')).every(r=>r.method==='POST')),true);
    await page.locator('[data-feedback-history]').first().click();
    await page.locator('dialog[open] [data-feedback-history-content] article').first().waitFor();
    assert.equal(await page.locator('dialog[open] [data-feedback-history-content] article').count(),2);
    assert.equal(await page.locator('dialog[open] img').count(),0);
    await page.locator('dialog[open] [name=homeDialogClose]').focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('dialog[open] [name=homeDialogClose]').evaluate(el=>el===document.activeElement),true);
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('[data-feedback-history]').first().evaluate(el=>el===document.activeElement),true,'close returns to the current opener after language redraw');
    await page.evaluate(()=>window.holdFeedbackHistory=true);
    await page.locator('[data-feedback-history]').first().click();
    await page.waitForFunction(()=>typeof window.releaseFeedbackHistory==='function');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>window.homeSignals.filter(r=>r.path.startsWith('/api/opportunities/feedback/history')).at(-1).signal.aborted),true);
    await page.evaluate(()=>{window.holdFeedbackHistory=false;window.releaseFeedbackHistory();});
    assert.equal(await page.locator('dialog[open]').count(),0,'late history response cannot reopen a closed panel');
    await page.evaluate(()=>{window.failFeedbackHistory=true;Fund.applyLanguage('zh-CN');});
    await page.locator('[data-feedback-history]').first().click();
    await page.locator('dialog[open] [role=alert]').waitFor();
    await page.evaluate(()=>Fund.applyLanguage('en'));
    assert.equal(await page.locator('dialog[open] [role=alert]').textContent(),await page.evaluate(()=>Fund.t('fundHomeRequestFailed')),'cached history errors follow the current language');
    await page.keyboard.press('Escape');
    await page.evaluate(()=>{window.failFeedbackHistory=false;Fund.applyLanguage('zh-CN');});
    await page.evaluate(async()=>{window.failGeneration=true;await Fund.home.refresh();});
    await page.locator('#home-status-bar').waitFor();
    await page.evaluate(()=>Fund.applyLanguage('en'));
    assert.equal(await page.locator('#home-status-bar').textContent(),await page.evaluate(()=>Fund.t('fundHomeHomeSnapshotCouldNotLoad')+Fund.t('fundHomeRequestFailed')),'failed generation remains localized without exposing internal details');
    await page.evaluate(async()=>{window.failGeneration=false;Fund.applyLanguage('zh-CN');await Fund.home.refresh();});

    await page.evaluate(()=>window.holdOpportunityFeedback=true);
    await page.locator('[data-feedback-reason]').first().fill('saved while pending');
    await page.locator('[data-feedback-useful=true]').first().click();
    await page.waitForFunction(()=>typeof window.releaseOpportunityFeedback==='function');
    await page.locator('[data-feedback-reason]').first().fill('new draft after send');
    await page.evaluate(()=>{window.holdOpportunityFeedback=false;window.releaseOpportunityFeedback();});
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-records]').disabled);
    assert.equal(await page.locator('[data-feedback-reason]').first().inputValue(),'new draft after send','late accepted write preserves subsequent user typing');
    assert.equal(await page.locator('[data-record-batch-select]').isDisabled(),true,'unsaved feedback stays within a bounded active batch');
    await page.locator('[data-feedback-reason]').first().fill('saved while pending');
    assert.equal(await page.locator('[data-record-batch-select]').isDisabled(),false);
    await page.evaluate(()=>window.failFeedbackReply=true);
    await page.locator('[data-feedback-useful=true]').first().click();
    await page.locator('.opportunity-records [role=alert]').waitFor();
    await page.evaluate(()=>window.failFeedbackReply=false);
    await page.locator('[data-feedback-useful=true]').first().click();
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-records]').disabled);
    const repeated=await page.evaluate(()=>window.requests.filter(r=>r.path==='/api/opportunities/feedback').slice(-2).map(r=>r.body.requestId));
    assert.equal(repeated[0],repeated[1],'lost write response retry reuses the exact owned requestId');
    assert.equal(await page.evaluate(()=>window.savedOpportunity.items[0].feedback.version),4,'idempotent retry does not add another feedback version');
    await page.locator('[data-opportunity-refresh]').click();
    await page.waitForFunction(()=>window.requests.some(r=>r.path.startsWith('/api/opportunities/status')));
    assert.equal(await page.locator('[data-opportunity-refresh]').isDisabled(),true);
    await page.locator('#home-opportunity-body [name=minScore]').fill('88');
    await page.locator('#home-opportunity-body [name=minScore]').focus();
    await page.evaluate(()=>document.dispatchEvent(new Event('fund:language-changed')));
    assert.equal(await page.locator('#home-opportunity-body [name=minScore]').inputValue(),'88');
    assert.equal(await page.locator('#home-opportunity-body [name=minScore]').evaluate(el=>el===document.activeElement),true);
    await page.evaluate(()=>{Fund.home.applyDisplayMode('hidden',false);Fund.home.renderOpportunities(Fund.home.snapshot()?.opportunities);});
    assert.equal(await page.locator('[data-opportunity-candidate]').count(),0);
    assert.equal(await page.locator('[data-opportunity-cancel]').count(),1,'hidden state retains a usable cancellation control');
    assert.equal(await page.locator('#home-opportunity-body').innerText().then(t=>t.includes('600519')),false);
    await page.evaluate(()=>{Fund.home.applyDisplayMode('amount',false);Fund.home.renderOpportunities(Fund.home.snapshot()?.opportunities);window.opportunityTask={...window.opportunityTask,status:'complete',completed:2,nextOffset:10};});
    await page.locator('[data-opportunity-next]').waitFor();
    await page.locator('[data-opportunity-next]').click();
    const continuation=await page.evaluate(()=>window.requests.filter(r=>r.path==='/api/opportunities/refresh').at(-1).body);
    assert.equal(continuation.offset,10);assert.equal(continuation.poolId,'a'.repeat(64));
    await page.evaluate(()=>{window.holdOpportunityStatus=true;});
    await page.waitForFunction(()=>!!window.releaseOpportunityStatus);
    await page.evaluate(()=>Fund.actions.leaveHome());
    await page.waitForFunction(()=>window.opportunityTask.status==='cancelled');
    await page.evaluate(()=>{window.holdOpportunityStatus=false;window.releaseOpportunityStatus();window.releaseOpportunityStatus=null;});
    await page.evaluate(()=>Fund.actions.loadHome());
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-refresh]').disabled);
    await page.evaluate(()=>{window.holdOpportunityStart=true;});
    await page.locator('[data-opportunity-refresh]').click();
    await page.waitForFunction(()=>!!window.releaseOpportunityStart);
    await page.evaluate(()=>{Fund.actions.leaveHome();window.holdOpportunityStart=false;window.releaseOpportunityStart();});
    await page.waitForFunction(()=>window.opportunityTask.status==='cancelled');
    await page.evaluate(()=>Fund.actions.loadHome());
    await page.waitForFunction(()=>!document.querySelector('[data-opportunity-refresh]').disabled);
    await page.evaluate(()=>{window.failOpportunityStatus=true;});
    await page.locator('[data-opportunity-refresh]').click();
    await page.waitForFunction(()=>window.opportunityTask.status==='cancelled');
    assert.equal(await page.locator('#home-opportunity-body [role=alert]').count(),1,'status error is explicit and stops the remote worker');
    await page.evaluate(()=>{window.failOpportunityStatus=false;});
    await page.evaluate(()=>{window.failOpportunityStart=true;});
    await page.locator('[data-opportunity-refresh]').click();
    await page.waitForFunction(()=>window.opportunityTask.status==='cancelled');
    assert.equal(await page.evaluate(()=>typeof window.requests.filter(r=>r.path==='/api/opportunities/cancel').at(-1).body.requestId),'string','a lost start reply is cancelled by its owned requestId');
    await page.evaluate(()=>{window.failOpportunityStart=false;});
    await page.locator('[data-opportunity-reset]').click();
    if(width===1280&&theme==='volt') {
      await page.evaluate(async()=>{window.failMarket=true;await Fund.actions.refreshHome();});
      await page.waitForFunction(()=>Fund.home.marketFailure()===true);
      assert.match(await page.locator('#home-market-body').textContent(),/3000\.00/,'refresh failure must retain dated last-good quote');
      assert.doesNotMatch(await page.locator('#home-market-time').textContent(),/15:00/,'headline must show failure instead of pretending refreshed success');
      await page.evaluate(async()=>{window.failMarket=false;await Fund.actions.refreshHome();});
      await page.waitForFunction(()=>Fund.home.marketFailure()===false);
      await page.evaluate(async()=>{window.holdBenchmark=true;window.bumpPreferenceRevision=true;document.getElementById('home-privacy-btn').click();});
      await page.waitForFunction(()=>typeof window.releaseBenchmark==='function');
      const benchmarkCount=await page.evaluate(()=>window.requests.filter(r=>r.path.startsWith('/api/portfolio/benchmark')).length);
      await page.locator('#home-privacy-btn').click();
      await page.locator('#home-privacy-btn').click();
      await page.waitForFunction(()=>Fund.home.snapshot().dataRevision==='10');
      assert.equal(await page.evaluate(()=>window.requests.filter(r=>r.path.startsWith('/api/portfolio/benchmark')).length),benchmarkCount,'revision changes must share one in-flight benchmark');
      await page.evaluate(()=>{window.holdBenchmark=false;window.releaseBenchmark();});
      await page.waitForFunction(()=>Fund.home.benchmark()?.dataRevision==='10');
      assert.equal(await page.evaluate(()=>window.requests.filter(r=>r.path.startsWith('/api/portfolio/benchmark')).length),benchmarkCount+1,'only latest revision is fetched after stale work completes');
      await page.evaluate(()=>window.bumpPreferenceRevision=true);
      for(let i=0;i<3;i++) await page.locator('#home-privacy-btn').click();
      await page.waitForFunction(()=>Fund.home.snapshot().dataRevision==='13'&&Fund.home.benchmark()?.dataRevision==='13');
      await page.evaluate(()=>window.bumpPreferenceRevision=false);
      // 离开与后台：请求取消，迟到不覆盖；已确认页面恢复时不重新取数。
      await page.evaluate(() => { window.holdHomeReads = true; Fund.actions.refreshHome(); });
      await page.waitForFunction(() => window.heldHomeRequests.length === 1);
      await page.evaluate(() => { Fund.actions.refreshHome(); });
      await page.waitForFunction(() => window.heldHomeRequests.length === 2);
      assert.equal(await page.evaluate(() => window.heldHomeRequests[0].signal.aborted),true);
      await page.evaluate(() => window.heldHomeRequests[0].resolve());
      await page.waitForTimeout(50);
      assert.equal(await page.evaluate(() => window.heldHomeRequests.length),2,'obsolete refresh must not start rules or indices');
      await page.evaluate(() => Fund.actions.leaveHome());
      assert.equal(await page.evaluate(() => window.heldHomeRequests[1].signal.aborted),true,'old finally must preserve the new controller until leave cancels it');
      await page.evaluate(() => { window.heldHomeRequests[1].resolve(); window.holdHomeReads = false; });
      await page.evaluate(() => Fund.actions.loadHome());
      await page.waitForFunction(() => document.getElementById('home-today-body').getAttribute('aria-busy')!=='true');
      await page.evaluate(() => { window.holdHomeReads = true; Fund.actions.refreshHome(); });
      await page.waitForFunction(() => window.heldHomeRequests.length === 3);
      await page.evaluate(() => {
        Object.defineProperty(document, 'hidden', { configurable:true, value:true });
        document.dispatchEvent(new Event('visibilitychange'));
      });
      assert.equal(await page.evaluate(() => window.heldHomeRequests[2].signal.aborted),true);
      const hiddenCount = await page.evaluate(() => window.requests.length);
      await page.evaluate(() => Fund.actions.refreshHome());
      assert.equal(await page.evaluate(() => window.requests.length),hiddenCount,'hidden page must not issue new reads');
      await page.evaluate(() => {
        window.heldHomeRequests[2].resolve(); window.holdHomeReads = false;
        Object.defineProperty(document, 'hidden', { configurable:true, value:false });
        document.dispatchEvent(new Event('visibilitychange'));
        window.dispatchEvent(new PageTransitionEvent('pageshow', {persisted:true}));
      });
      await page.waitForTimeout(50);
      assert.equal(await page.evaluate(n => window.requests.slice(n).filter(r => /\/api\/home\/snapshot|\/api\/check\/run|\/api\/market\/indices/.test(r.path)).length,hiddenCount),0,'visibility and BFCache restore preserve the confirmed page without refreshing');
    }

    // ---- 方案 11 §4 分组面板：draft 输入在新增行后保留；保存生效；409 冲突保留草稿 ----
    await page.locator('#home-group-manage').click();
    await page.locator('dialog[open] form').waitFor();
    await page.locator('dialog[open] [data-add]').click();
    await page.locator('dialog[open] [name=name-0]').fill('Equity Group');
    await page.locator('dialog[open] [name=desc-0]').fill('Stocks and exchange-traded funds');
    await page.locator('dialog[open] [name=ratio-0]').fill('60');
    await page.locator('dialog[open] [data-add]').click();
    assert.equal(await page.locator('dialog[open] [name=name-0]').inputValue(), 'Equity Group', 'syncDraft must keep input across rerender');
    assert.equal(await page.locator('dialog[open] [name=ratio-0]').inputValue(), '60');
    assert.equal(await page.locator('dialog[open] [name=desc-0]').inputValue(), 'Stocks and exchange-traded funds');
    await page.locator('dialog[open] [name=name-1]').fill('Cash Group');
    await page.locator('dialog[open] [name=desc-1]').fill('Cash available for allocation');
    await page.locator('dialog[open] [name=ratio-1]').fill('40');
    await page.locator('dialog[open] [type=submit]').click();
    await page.waitForFunction(() => window.requests.some(r => r.path === '/api/home/group-plan' && r.method === 'PUT'));
    const plan = await page.evaluate(() => window.requests.find(r => r.path === '/api/home/group-plan' && r.method === 'PUT').body);
    assert.deepEqual(plan.groups.map(g => [g.name, g.ratioTenths]), [['Equity Group', 600], ['Cash Group', 400]]);
    assert.deepEqual(plan.groups.map(g => g.description), ['Stocks and exchange-traded funds', 'Cash available for allocation']);
    assert.equal(plan.expectedRevision, 0);
    await page.locator('dialog[open]').waitFor({ state: 'detached' });
    await page.locator('#home-group-manage').click();
    await page.locator('dialog[open] form').waitFor();
    await page.locator('dialog[open] [name=name-0]').fill('Renamed Group');
    await page.evaluate(() => { window.planConflict = true; });
    await page.locator('dialog[open] [type=submit]').click();
    await page.waitForFunction(() => (document.querySelector('dialog .home-form-error')?.textContent || '').length > 0);
    assert.equal(await page.locator('dialog[open] [name=name-0]').inputValue(), 'Renamed Group', '409 must keep the draft');
    assert.equal(await page.locator('dialog[open]').count(), 1, '409 must keep the panel open');
    await page.evaluate(() => { window.planConflict = false; });
    await page.keyboard.press('Escape');

    // ---- R08 竞态：挂起快照+迟到放行，percent 下无论渲染哪个快照都不得显露金额 ----
    // （runCheckAndRender 会再触发一次 loadSnapshot，多个挂起请求由同一次放行 resolve——
    //   断言隐私不变量而非特定数值，避免与内部时序耦合。）
    await page.evaluate(() => { window.slowSnapshot = true; });
    // 不 await：refreshHome 的 promise 被挂起的快照 fetch 阻塞，等待由 releaseSnapshot 放行。
    void page.evaluate(() => { Fund.actions.refreshHome(); });
    await page.locator('#home-privacy-btn').click();
    assert.equal(await page.locator('#hc-total-value').textContent(), '****');
    await page.evaluate(() => { window.slowSnapshot = false; window.releaseSnapshot(); });
    await page.waitForTimeout(800);
    assert.equal(await page.locator('#hc-total-value').textContent(), '****', 'late snapshot must not reveal amounts in percent mode');
    assert.doesNotMatch(await page.locator('#home-distribution-body').textContent(), /12,345,678|61,728/, 'percent DOM must stay amount-free after late response');
    await page.locator('#home-ai-explain').click();
    try {
      await page.locator('dialog[open] form').waitFor({ timeout: 8000 });
    } catch (e) {
      throw new Error(`percent AI panel failed; status=${await page.locator('#home-status-bar').textContent()}; dialogs=${await page.locator('dialog[open]').count()}`);
    }
    assert.equal(await page.locator('dialog[open] [name=discloseFinance]').isDisabled(), true, 'percent mode must disable amount disclosure');
    await page.keyboard.press('Escape');
    await page.locator('#home-privacy-btn').click();
    await page.locator('#home-privacy-btn').click();
    await page.waitForFunction(() => document.querySelector('#hc-total-value')?.textContent !== '****');

    if(width===1280 && theme==='archive') {
      await page.evaluate(async()=>{window.researchQuery='sh600519:day:qfq';await Fund.home.openAI({research:{identity:()=>researchQuery,capture:()=>({identity:{symbol:'sh600519',period:'dayk',fq:'qfq',seriesKind:'OHLC',range:{start:0,end:100}},rows:[{date:'2026-09-01',open:10,close:11}],source:'synthetic-public-series',capturedAt:'2026-09-01T07:00:00Z'})}});});
      assert.equal(await page.locator('dialog[open] [name=scopeAsset]').inputValue(),'');
      assert.equal(await page.locator('dialog[open] [data-send]').isDisabled(),true);
      await page.locator('dialog[open] form [type=submit]').click();
      await page.waitForFunction(()=>window.lastEvidenceBody?.scope?.asset==='research:sh600519');
      assert.equal(await page.evaluate(()=>lastEvidenceBody.discloseFinance),false);
      assert.equal(await page.evaluate(()=>lastEvidenceBody.research.rows[0].date),'2026-09-01');
      await page.locator('dialog[open] [data-send]:not([disabled])').waitFor();
      assert.equal(await page.locator('dialog[open] [data-send]').isDisabled(),false);
      await page.evaluate(()=>{researchQuery='sz000001:day:qfq';document.dispatchEvent(new Event('fund:research-selection-changed'));});
      assert.equal(await page.locator('dialog[open] [data-send]').isDisabled(),true);
      assert.equal(await page.locator('dialog[open] [data-preview]').innerText(),'');
      await page.keyboard.press('Escape');
    }
    await page.locator('#home-goal-manage').click();
    assert.equal(await page.locator('dialog[open] h2').textContent(), '财务目标设置');
    await page.locator('dialog[open] [data-close]').click();
    await page.locator('#home-cash-manage').click();
    assert.equal(await page.locator('dialog[open] h2').textContent(), '现金与资金核对');
    await page.locator('dialog[open] [data-close]').click();
    await page.locator('#home-today-rules').click();
    await page.locator('dialog[open] [data-create]').click();
    await page.locator('dialog[open] [name=ruleType]').selectOption('drawdown');
    await page.locator('dialog[open] [name=name]').fill('Synthetic drawdown');
    await page.locator('dialog[open] [name=objectKey]').fill('security:SSE:600519');
    await page.locator('dialog[open] [name=threshold]').fill('12.5');
    await page.locator('dialog[open] [name=window]').fill('60');
    await page.locator('dialog[open] [type=submit]').click();
    await page.waitForFunction(() => window.requests.some(r => r.path === '/api/rules' && r.body));
    const rule = await page.evaluate(() => window.requests.find(r => r.path === '/api/rules' && r.body).body);
    assert.deepEqual(rule.params, { threshold: 12.5, window: 60 });
    await page.locator('dialog[open] [data-close]').click();
    await page.evaluate(async () => { window.postMessage({ type: 'welcome', generation: 'synthetic-generation', challenge: 'synthetic-challenge' }, '*'); await new Promise(resolve => setTimeout(resolve, 0)); });
    await page.locator('#home-ai-explain').click();
    await page.waitForTimeout(100);
    assert.equal(await page.locator('dialog[open] form').count(), 1, await page.locator('#home-status-bar').textContent());
    await page.locator('dialog[open] [type=submit]').click();
    await page.locator('dialog[open] [data-preview] details > summary').click().catch(async error=>{console.log('AI preview state',await page.evaluate(()=>({mode:Fund.home.displayMode(),text:document.querySelector('dialog[open] [data-preview]')?.textContent,error:document.querySelector('dialog[open] .home-form-error')?.textContent})));throw error;});
    await page.locator('dialog[open] [data-preview]').getByText('"E1"').waitFor();
    assert.equal(await page.locator('dialog[open] [data-preview]').textContent().then(text => text.includes('Private Account')), false);
    await page.locator('dialog[open] [data-send]').click();
    await page.locator('dialog[open] [data-result]').getByText('Synthetic AI explanation').waitFor();
    await page.evaluate(() => { window.slowStatus = true; });
    await page.locator('dialog[open] [data-send]').click();
    await page.waitForFunction(() => window.delayedReplies.length === 1);
    await page.locator('dialog[open] [data-cancel]').click();
    await page.evaluate(() => { window.slowStatus = false; });
    await page.locator('dialog[open] [data-send]').click();
    await page.evaluate(() => window.delayedReplies.shift()());
    await page.locator('dialog[open] [data-result]').getByText('Synthetic AI explanation').waitFor();
    // 方案 05 AI 四类任务：盘前／盘后简报——任务切换显隐市场/时段字段，POST 携带简报参数且不带单项范围。
    await page.locator('dialog[open] [name=task]').selectOption('briefing');
    assert.equal(await page.locator('dialog[open] [data-briefing]').evaluate(el => el.hidden), false, 'briefing fields appear for briefing task');
    await page.locator('dialog[open] [name=briefingMarket]').selectOption('HK');
    await page.locator('dialog[open] [name=briefingSession]').selectOption('post');
    await page.locator('dialog[open] [type=submit]').click();
    await page.locator('dialog[open] [data-preview] details > summary').click();
    await page.locator('dialog[open] [data-preview]').getByText('"briefing"').waitFor();
    const briefingBody = await page.evaluate(() => window.lastEvidenceBody);
    assert.equal(briefingBody.task, 'briefing');
    assert.equal(briefingBody.briefingMarket, 'HK', 'briefing market must be sent');
    assert.equal(briefingBody.briefingSession, 'post', 'briefing session must be sent');
    assert.equal('scope' in briefingBody, false, 'briefing must not carry a single-asset scope');
    await page.locator('dialog[open] [name=task]').selectOption('diagnosis');
    assert.equal(await page.locator('dialog[open] [data-briefing]').evaluate(el => el.hidden), true, 'briefing fields hide for other tasks');
    await page.keyboard.press('Escape');
    // P02 简化目标（08 最新）：弹窗只有金额输入；语言切换保留草稿金额。
    await page.locator('#home-goal-manage').click();
    await page.locator('dialog[open] [data-edit]').click();
    await page.locator('dialog[open] [name=target]').fill('123456.78');
    await page.evaluate(() => Fund.applyLanguage('en'));
    assert.equal(await page.locator('dialog[open] h2').textContent(), 'Edit financial goal');
    assert.equal(await page.locator('dialog[open] [name=target]').inputValue(), '123456.78');
    assert.equal(await page.locator('#hc-total .hc-label').textContent(), 'Managed assets');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(() => document.activeElement.id), 'home-goal-manage');
    await page.locator('#home-today-rules').click();
    await page.locator('dialog[open] [data-create]').click();
    await page.locator('dialog[open] [name=ruleType]').selectOption('strategy_exit');
    await page.locator('dialog[open] [name=condition]').selectOption('price');
    await page.locator('dialog[open] [name=threshold]').fill('99.5');
    await page.evaluate(() => Fund.applyLanguage('zh-CN'));
    assert.equal(await page.locator('dialog[open] [name=ruleType]').inputValue(), 'strategy_exit');
    assert.equal(await page.locator('dialog[open] [name=condition]').inputValue(), 'price');
    assert.equal(await page.locator('dialog[open] [name=threshold]').inputValue(), '99.5');
    await page.keyboard.press('Escape');
    await page.evaluate(() => Fund.applyLanguage('en'));
    for (const selector of ['#home-cash-manage', '#home-allocation-manage', '#home-sources-manage', '#home-simulate']) {
      await page.locator(selector).click();
      await page.locator('dialog[open]').waitFor();
      const text = await page.locator('dialog[open]').textContent();
      assert.doesNotMatch(text, /[\u4e00-\u9fff]/, text);
      assert.ok(await page.evaluate(() => getComputedStyle(document.querySelector('dialog')).color !== 'rgb(238, 238, 238)'));
      await page.keyboard.press('Tab');
      assert.ok(await page.evaluate(() => !!document.activeElement.closest('dialog')));
      await page.keyboard.press('Escape');
    }
    // ---- 编辑版本冲突：后端 409 的 message 必须出现在 dialog 错误区（P02 简化后编辑表单只有金额） ----
    await page.locator('#home-goal-manage').click();
    await page.locator('dialog[open] [data-edit]').first().click();
    await page.locator('dialog[open] [name=target]').fill('300000');
    // 走真实 onsubmit → api() → 409 → failure 展示链路。
    await page.evaluate(() => document.querySelector('dialog form').requestSubmit());
    await page.waitForFunction(() => (document.querySelector('dialog .home-form-error')?.textContent || '').length > 0, undefined, { timeout: 5000 });
    const errText = await page.locator('dialog[open] .home-form-error').textContent();
    assert.match(errText, /version conflict|record changed|冲突|记录.*变化/i, `409 message must surface, got: ${errText}`);
    assert.equal(await page.locator('dialog[open] [type=submit]').isEnabled(), true, 'conflict must not disable the form permanently');
    await page.keyboard.press('Escape');

    // ---- 200% 缩放：无横向溢出，dialog 不超出视口 ----
    await page.evaluate(() => { document.documentElement.style.zoom = '2'; });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `200% overflow: ${theme}/${width}`);
    await page.locator('#home-goal-manage').click();
    await page.locator('dialog[open]').waitFor();
    if (width === 1280) await page.screenshot({ path: resolve(shots, `${theme}-en-200.png`), fullPage: true });
    assert.ok(await page.evaluate(() => { const r = document.querySelector('dialog').getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth + 1; }), `200% dialog overflow: ${theme}/${width}`);
    await page.keyboard.press('Escape');
    await page.evaluate(async()=>{
      document.documentElement.style.zoom='';Fund.home.applyDisplayMode('amount',false);
      window.contentFetchBackup=window.fetch;
      window.fetch=(url,options)=>String(url).includes('/api/information/item?')?Promise.resolve(new Response(JSON.stringify({id:1,title:'Content failure fixture',sourceId:'em_stock_ann',completeness:'link_only',contentVersion:0,relations:[]}))):String(url).endsWith('/api/information/content')?Promise.resolve(new Response(JSON.stringify({error:'INFO_SOURCE_FETCH_FAILED'}),{status:503})):contentFetchBackup(url,options);
      await Fund.home.openInformationDetail(1);
    });
    await page.locator('dialog[open] [data-info-fetch]').click();
    await page.locator('dialog[open] .home-form-error').filter({hasText:/\S/}).waitFor();
    assert.equal(await page.locator('dialog[open] [data-info-fetch]').isEnabled(),true,'content failure keeps a visible retry action');
    await page.keyboard.press('Escape');
    await page.evaluate(()=>{window.fetch=contentFetchBackup;delete window.contentFetchBackup;});
    assert.deepEqual(errors, []);
    await page.close();
  }

  // ---- 状态矩阵渲染：incomplete / stale / empty 快照，未知不显示 0 ----
  {
    const page2 = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const css = ['theme', 'layout', 'panel', 'home', 'investment'].map(name => readFileSync(resolve(name === 'theme' ? resolve(root, '../dist') : root, `styles/${name}.css`), 'utf8')).join('\n');
    await page2.setContent(`<style>${css}</style>${readFileSync(resolve(root, 'partials/view-home.html'), 'utf8')}`);
    await page2.evaluate(() => { document.getElementById('view-home').hidden = false; });
    await page2.evaluate(() => {
      const snapshot = {
        snapshotId: 'home-synthetic-9', dataRevision: '9', generatedAt: 1790740800,
        wealth: { quality: 'incomplete', issues: [{ code: 'missing_price', count: 2 }, { code: 'stale_valuation', count: 1 }], totalA: 500000, investedI: null, latestValuation: '2026-09-20',
          returns: { status: 'incomplete', reason: 'missing_baseline', netReturnMinor: null },
          cash: { cashMinor: null, reserveState: 'unconfigured', reservedMinor: null, reservedByOrdersMinor: 0, availableMinor: null, shortfallMinor: null, accounts: [] } },
        goals: { status: 'empty' },
        allocation: { configured: true, computable: false, reason_codes: ['unknown_class'], classes: [] },
        alerts: { openCount: 0, lastCheck: null },
      };
      window.fetch = async (url) => new Response(JSON.stringify(
        String(url).startsWith('/api/home/snapshot') ? snapshot
        : String(url).startsWith('/api/market/indices') ? { quotes: [] }
        : { ok: true }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    });
    for (const file of ['core.js', 'home-i18n.js', 'home-overview.js', 'home.js']) await page2.addScriptTag({ path: resolve(root, file) });
    await page2.evaluate(() => { Fund.state.token = 'synthetic'; Fund.actions.loadHome(); });
    await page2.locator('#home-finance-details summary').click();
    await page2.locator('.home-history > summary').click();
    await page2.waitForTimeout(200);
    const text = await page2.locator('#view-home').innerText();
    assert.match(text, /待核实/, 'incomplete wealth must show 待核实, not 0');
    assert.match(text, /暂无完整收益记录/, 'missing baseline return must be explicit');
    assert.match(text, /未建账/, 'unconfigured cash must not read as 0');
    assert.match(text, /还没有财务目标/, 'empty goal state guide');
    assert.doesNotMatch(text, /管理资产\s*0/, 'incomplete total must never render as bare zero');
    await page2.close();
    console.log('status matrix rendering (incomplete/stale/empty/unconfigured) passed');
  }
  console.log('fund home Chrome: privacy, forms, AI preview, live language with preserved drafts, keyboard/Escape, Volt/Archive, 1280/640/320px and 200% zoom passed');

  // ---- ECharts 按需加载（05 §8 初始预算 350KiB）：动态注入、单次、失败重试 ----
  {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    const echartsBytes = readFileSync(resolve(root, 'echarts.min.js'));
    let served = 0;
    let failFirst = true;
    await page.route('**/echarts.min.js', route => {
      served += 1;
      if (failFirst && served === 1) { failFirst = false; return route.fulfill({ status: 500, body: 'fixture failure' }); }
      return route.fulfill({ status: 200, contentType: 'application/javascript', body: echartsBytes });
    });
    await page.setContent('<html><head><base href="http://127.0.0.1:7/"></head><body></body></html>');
    await page.addScriptTag({ path: resolve(root, 'core.js') });
    await page.evaluate(() => { globalThis.Fund = globalThis.Fund; });
    // 首次请求失败 → 可重试；第二次成功 → echarts 可用且不重复注入。
    const first = await page.evaluate(() => Fund.ensureEcharts().then(() => 'ok', e => 'rejected:' + e.message));
    assert.match(first, /rejected:图表库加载失败/, 'first load failure must be retriable');
    const second = await page.evaluate(() => Fund.ensureEcharts().then(() => 'ok', e => 'rejected:' + e.message));
    assert.equal(second, 'ok', `second load must succeed after retry (served=${served})`);
    assert.ok(await page.evaluate(() => typeof window.echarts === 'object' || typeof window.echarts === 'function'));
    await page.evaluate(() => Fund.ensureEcharts());
    assert.equal(served, 2, 'echarts must be injected exactly once after failure recovery');
    assert.deepEqual(errors, []);
    await page.close();
    console.log('echarts lazy-load: dynamic injection, single fetch, failure retry passed');
  }
} finally {
  await browser.close();
}
