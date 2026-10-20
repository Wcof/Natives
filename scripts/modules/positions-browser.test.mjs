// 真实 Chrome 渲染器＋合成 API，验证持仓投影竞态；不访问真实账本或供应商。
import assert from 'node:assert/strict';
import { readFileSync,mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from 'playwright-core';
import { verifyPositionsMenu } from './positions-menu-browser.mjs';

const root = resolve(import.meta.dirname, '../..');
const ui = resolve(root, 'modules/fund/ui/src');
const messages = Object.fromEntries(['zh_CN', 'en'].map(lang => [lang,
  JSON.parse(readFileSync(resolve(root, `extension/_locales/${lang}/messages.json`), 'utf8'))]));
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 640, height: 800 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.setContent(`<style>${['theme', 'layout', 'panel', 'positions','investment'].map(n => readFileSync(resolve(n==='theme'?resolve(root,'modules/fund/ui/dist'):ui, `styles/${n}.css`), 'utf8')).join('\n')}</style>${readFileSync(resolve(ui, 'partials/view-positions.html'), 'utf8')}`);
  await page.evaluate(messages => {
    document.getElementById('view-positions').hidden = false;
    window.replies = []; window.calls = []; window.aiInput = null;
    const holding = (kind, id, code) => ({ accountId: 1, fundId: id, fundCode: code, fundName: `private-${kind}`, instrumentKind: kind,
      venue: kind === 'fund' ? 'otc' : 'exchange', quoteSymbol: kind === 'stock' ? 'sh600519' : null,
      quantity: '0.0123', cost: '0', realized: '0', costUnknown: false, marketValue: kind === 'fund' ? '0.01' : null,
      nav: kind === 'fund' ? { nav: '1.2345', date: new Date(Date.now()+8*3600000).toISOString().slice(0,10) } : null, pendingCount: 0, asOf: '2026-10-01' });
    window.snapshot = { dataRevision: '1', accounts: [{ id: 1, name: '<img src=x onerror="window.xss=true">', version: 1, archived: false }],
      holdings: [holding('stock', 1, '600519'), holding('fund', 2, '012414'),holding('etf',3,'510300')] };
    const state = { watchlistItems: [{symbol: 'sh600519', name: 'watch-private'}], language: 'zh-CN', displayMode: 'amount' };
    window.Fund = {
      state, actions: {}, t: (key,values={}) => (messages[state.language === 'en' ? 'en' : 'zh_CN'][key]?.message || key).replace(/\{(\w+)\}/g,(_,name)=>values[name]??`{${name}}`),
      decimalToMinor: text => { const value=String(text),[a,b='']=value.replace(/^-/, '').split('.');return (BigInt(a)*100n+BigInt(b.padEnd(2,'0')))*(value.startsWith('-')?-1n:1n); },
      formatMinorExact: value => {const n=value<0n?-value:value;return `${value<0n?'-':''}${n/100n}.${String(n%100n).padStart(2,'0')}`;},
      home: { state, sensitive: () => false, openAI: input => { window.aiInput = input; },
        displayMode: () => state.displayMode,
        applyDisplayMode: (mode) => { state.displayMode = mode; document.dispatchEvent(new Event('fund:privacy-changed')); } },
      api: async (_method, url, _body, options) => {
        window.calls.push(url);
        if(url.startsWith('/api/fund/classification?'))return {code:url.split('=')[1],status:'ready',themes:[{id:'BK000054',name:'Semiconductors'}],sourceUrl:'https://fund.eastmoney.com/'+url.split('=')[1]+'.html',fetchedAt:Date.now()/1000};
        if (url === '/api/nav/sync' && window.navError) throw new Error('isolated NAV failure');
        if (url === '/api/nav/sync' && window.navBlocked) return await new Promise((resolve,reject)=>{window.navReply={resolve,reject,signal:options.signal};});
        if (window.blockDetail || url.includes('refresh=1')) return await new Promise((resolve, reject) => { window.replies.push({ url, signal: options.signal, resolve, reject }); });
        return structuredClone(window.snapshot);
      },
    };
  }, messages);
  const coreSource=readFileSync(resolve(ui,'core.js'),'utf8');
  const navClient=coreSource.slice(coreSource.indexOf('// Shared NAV sync owner:'),coreSource.indexOf('// \u6302\u8f7d\u547d\u540d\u7a7a\u95f4'));
  await page.addScriptTag({content:'(()=>{const {api}=Fund;'+navClient+'Fund.syncHoldingNav=syncHoldingNav;})();'});
  await page.addScriptTag({ path: resolve(ui, 'positions-menu.js') });
  await page.addScriptTag({ path: resolve(ui, 'positions-income.js') });
  await page.addScriptTag({ path: resolve(ui, 'fund-classification.js') });
  await page.addScriptTag({ path: resolve(ui, 'positions-view.js') });
  await page.evaluate(()=>{window.blockDetail=true;Fund.actions.loadPositions();});
  await page.waitForFunction(()=>replies.length===1);
  await page.evaluate(()=>{void Fund.actions.refreshPositionsQuotes();});
  assert.equal(await page.evaluate(()=>replies[0].signal.aborted),false,'quote ticks must not cancel the initial authoritative ledger read');
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,get:()=>window.testHidden});window.testHidden=true;document.dispatchEvent(new Event('visibilitychange'));});
  assert.equal(await page.evaluate(()=>replies[0].signal.aborted),true);
  await page.evaluate(()=>{window.testHidden=false;document.dispatchEvent(new Event('visibilitychange'));});
  assert.equal(await page.evaluate(()=>calls.filter(url=>url==='/api/positions/detail').length),2,'interrupted first read must resume when visible, rather than leaving holdings empty');
  await page.evaluate(()=>{Fund.actions.leavePositions();replies.splice(0).forEach(reply=>reply.resolve(structuredClone(snapshot)));calls.length=0;delete document.hidden;});
  await page.evaluate(() => { window.blockDetail = true; Fund.actions.loadPositions(); });
  await page.waitForFunction(() => replies.length === 1);
  assert.equal(await page.locator('#holding-add').isDisabled(), true, 'holding form waits for authoritative accounts');
  await page.evaluate(() => { window.blockDetail = false; replies.shift().resolve(structuredClone(snapshot)); });
  await page.waitForFunction(() => replies.length === 1);
  assert.equal(await page.locator('#holding-add').isEnabled(), true);
  assert.equal(await page.locator('.pos-holding-card:not(.pos-add-card)').count(),3,'one card per stock, OTC fund and ETF');
  assert.equal(await page.locator('.pos-add-card').count(),1,'add-card placeholder precedes holdings');
  assert.equal(await page.locator('.pos-management').evaluate(node=>node.tagName),'SECTION','management and totals must be permanently visible, not a disclosure');
  for(const id of ['pos-total-cost','pos-total-value','pos-total-day-pnl','pos-accounts','pos-refresh'])assert.equal(await page.locator('#'+id).isVisible(),true);
  await page.waitForFunction(()=>document.querySelectorAll('.fund-theme-tag').length===2);
  assert.equal(await page.locator('.pos-holding-card [data-fund-classification]').count(),2,'funds and ETFs have public theme tags; stocks do not');
  const estimate=await page.evaluate(()=>{const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);window.estimateFixture={income:{day:{amount:null,date},estimatedDay:{date,amount:'10',rateBps:'500',estimated:true,coverageBps:5000}}};return Fund.positionIncome.inline(estimateFixture,'day');});
  assert.match(estimate,/10.00/);assert.match(estimate,/5.00%/);assert.match(estimate,/50.00%/);
  assert.doesNotMatch(await page.evaluate(()=>{estimateFixture.income.day.amount='3';return Fund.positionIncome.inline(estimateFixture,'day');}),/10.00/,'confirmed income wins over estimates');
  assert.equal(await page.locator('.pos-table').count(),0);
  assert.equal(await page.locator('.pos-card-details[open]').count(),0);
  await page.locator('#pos-detailed').check();
  assert.equal(await page.locator('.pos-card-details[open]').count(),3);
  await page.locator('#pos-detailed').uncheck();
  assert.equal(await page.locator('.pos-card-details[open]').count(),0);
  await page.locator('.pos-card-details > summary').first().press('Enter');
  assert.equal(await page.locator('.pos-card-details[open]').count(),1);

  for(const key of ['week','month','year','cumulative'])assert.ok((await page.locator(`.pos-metrics [data-income=${key}] > small`).first().innerText()).length>0,'each secondary income value has a visible label');
  assert.equal(await page.locator('.pos-watch, .pos-chip, [data-pos-watch-open]').count(), 0);
  assert.equal(await page.evaluate(() => Fund.state.watchlistItems.length), 1, 'removing chips preserves watchlist data');
  await page.click('#pos-tab-otc');
  await page.waitForFunction(() => replies.length === 2);
  assert.equal(await page.evaluate(() => replies[0].signal.aborted), true);
  await page.evaluate(() => replies[1].resolve(structuredClone(snapshot)));
  await page.waitForFunction(() => document.getElementById('pos-total-value').textContent === '0.01');
  await page.evaluate(() => replies[0].resolve({ ...structuredClone(snapshot), dataRevision: 'old', holdings: [] }));
  await page.waitForTimeout(50);
  assert.match(await page.locator('#positions-groups').innerText(), /012414/);
  assert.equal(await page.locator('#pos-total-value').innerText(), '0.01');
  await page.locator('.pos-card-details > summary').first().click();
  assert.match(await page.locator('#positions-groups').innerText(), /0\.0123/);
  assert.match(await page.locator('#positions-groups').innerText(), /1\.2345/);
  await page.click('.pos-more > summary');
  await page.click('[data-pos-act=ai]');
  assert.deepEqual(await page.evaluate(() => aiInput), { asset: 'fund:1:2' });
  assert.equal(await page.locator('.pos-more[open]').count(), 0, 'selecting an action dismisses the menu');
  await page.click('.pos-more > summary');
  await page.locator('[data-pos-act=ai]').focus();
  await page.evaluate(() => { Fund.actions.refreshPositionsQuotes(); });
  await page.waitForFunction(() => replies.length === 3);
  await page.evaluate(() => replies[2].resolve(structuredClone(snapshot)));
  await page.waitForFunction(() => document.querySelector('.pos-more').open && document.activeElement.dataset.posAct === 'ai');
  await verifyPositionsMenu(page);
  await page.locator('.pos-more > summary').first().click();
  await page.evaluate(() => { Fund.home.applyDisplayMode('hidden', false); document.dispatchEvent(new Event('fund:privacy-changed')); });
  assert.equal(await page.locator('.pos-action-menu:popover-open').count(), 0, 'privacy broadcast dismisses an open menu');
  const hidden = await page.locator('#view-positions').innerText();
  assert.doesNotMatch(hidden, /012414|private-fund|0\.0123|0\.01|600519/);
  await page.evaluate(() => { Fund.home.applyDisplayMode('amount', false); Fund.state.language = 'en'; document.dispatchEvent(new Event('fund:language-changed')); });
  await page.locator('#pos-detailed').check();
  assert.match(await page.locator('#positions-groups').innerText(), /Units/);
  assert.doesNotMatch(await page.locator('#positions-groups').innerText(), /[\u4e00-\u9fff]/);
  assert.equal(await page.evaluate(() => window.xss || false), false);
  await page.locator('#pos-tab-otc').press('ArrowLeft');
  assert.equal(await page.locator('#pos-tab-etf').getAttribute('aria-selected'), 'true', 'arrow roving crosses the new etf tab');
  await page.locator('#pos-tab-etf').press('ArrowLeft');
  assert.equal(await page.locator('#pos-tab-stock').getAttribute('aria-selected'), 'true');
  await page.locator('.pos-more > summary').first().click();
  await page.evaluate(() => { window.blockDetail = true; Fund.actions.loadPositions(); });
  await page.waitForFunction(() => replies.at(-1).url === '/api/positions/detail');
  await page.evaluate(() => replies.at(-1).reject(new Error('isolated account load failure')));
  await page.locator('#positions-groups .err').waitFor();
  assert.equal(await page.locator('.pos-action-menu:popover-open').count(), 0, 'failed loading cannot retain detached menu');
  assert.equal(await page.locator('#holding-add').isDisabled(), true, 'failed snapshot cannot open a stale account form');
  await page.evaluate(() => { window.blockDetail = false; Fund.actions.loadPositions(); });
  await page.waitForFunction(() => !document.getElementById('holding-add').disabled);
  await page.evaluate(() => { Fund.actions.leavePositions(); document.getElementById('view-positions').hidden = true; });
  assert.equal(await page.evaluate(() => replies.at(-1).signal.aborted), true);
  // 显示开关：开＝数字（amount），关＝星号（hidden），经 home.applyDisplayMode 单一 owner。
  const displayToggle = page.locator('#pos-display-toggle');
  await page.evaluate(() => { document.getElementById('view-positions').hidden = false; Fund.actions.loadPositions(); });
  await page.waitForFunction(() => !document.getElementById('holding-add').disabled);
  assert.equal(await displayToggle.getAttribute('aria-checked'), 'true', 'toggle reflects amount mode');
  assert.match(await displayToggle.textContent(), /Hide amounts/);
  await displayToggle.click();
  assert.equal(await displayToggle.getAttribute('aria-checked'), 'false');
  assert.doesNotMatch(await page.locator('#positions-groups').innerText(), /0\.0123/);
  await displayToggle.click();
  assert.equal(await displayToggle.getAttribute('aria-checked'), 'true');
  await page.locator('.pos-card-details > summary').first().press('Enter');
  assert.match(await page.locator('#positions-groups').innerText(), /0\.0123/);
  // 恢复既有断言的前置状态：视图隐藏，行情刷新被跳过。
  await page.evaluate(() => { Fund.actions.leavePositions(); document.getElementById('view-positions').hidden = true; });
  const requests = await page.evaluate(() => calls.length);
  await page.evaluate(() => Fund.actions.refreshPositionsQuotes());
  assert.equal(await page.evaluate(() => calls.length), requests);
  // 卡牌视觉与布局回归：示例投影，不代表真实用户持仓。
  const out=resolve(root,'.tmp/positions-cards');mkdirSync(out,{recursive:true});
  await page.evaluate(()=>{
    document.getElementById('view-positions').hidden=false;
    snapshot.accounts[0].name='主账户';
    const names=['贵州茅台','易方达优质精选混合 C','沪深 300 ETF'];
    snapshot.holdings.forEach((h,i)=>{
      h.fundName=names[i];h.marketValue=['18600.00','12380.50','8500.00'][i];h.quantity=['10','8015.8624','2000'][i];h.cost=['18000','12000','8100'][i];
      h.averageCost=['1800','1.4970','4.0500'][i];h.holdingDays=32;
      h.income=Object.fromEntries(['day','yesterday','holding','week','month','year','cumulative'].map(key=>[key,{amount:key==='yesterday'?'-28.30':'380.50',rateBps:key==='yesterday'?-23:317,date:key==='yesterday'?'2026-10-05':'2026-10-06'}]));
      h.quote=h.venue==='exchange'?{price:i===0?'1860.00':'4.2500',asOf:'2026-10-06'}:null;
    });
    snapshot.dataRevision='cards-visual';Fund.state.language='zh-CN';Fund.home.applyDisplayMode('amount');Fund.actions.loadPositions();
  });
  await page.waitForFunction(()=>document.querySelector('.pos-identity h3')?.textContent==='贵州茅台');
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  await page.locator('#pos-tab-all').click();
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  await page.locator('#pos-detailed').check();
  await page.locator('#pos-detailed').uncheck();
  for(const theme of ['archive','volt'])for(const language of ['zh-CN','en'])for(const width of [320,640,1280]){
    await page.setViewportSize({width,height:1100});
    await page.evaluate(({theme,language})=>{document.documentElement.dataset.theme=theme;Fund.state.language=language;document.dispatchEvent(new Event('fund:language-changed'));document.getElementById('view-positions').scrollTop=0;}, {theme,language});
    assert.equal(await page.locator('.pos-holding-card:not(.pos-add-card)').count(),3);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),'cards never overflow the page');
    assert.equal(await page.locator('.pos-card-daily [data-income]').count(),6);
    const cardStyle=await page.locator('.pos-add-card').evaluate(el=>{const c=getComputedStyle(el),article=el.parentElement.querySelector('article'),other=getComputedStyle(article),a=el.getBoundingClientRect(),b=article.getBoundingClientRect();return {radius:c.borderRadius,otherRadius:other.borderRadius,border:c.borderStyle,height:a.height,otherHeight:b.height,top:a.top,otherTop:b.top};});
    assert.equal(cardStyle.radius,cardStyle.otherRadius);assert.equal(cardStyle.border,'dashed');assert.ok(cardStyle.height>=280);
    if(width>=1280)assert.equal(cardStyle.top,cardStyle.otherTop,'wide cards share a row');
    if(cardStyle.top===cardStyle.otherTop)assert.ok(Math.abs(cardStyle.height-cardStyle.otherHeight)<2,'add card matches its row height: '+JSON.stringify({theme,language,width,...cardStyle}));

    await page.screenshot({path:resolve(out,`cards-${theme}-${language}-${width}.png`)});
  }
  // 非空旧净值也必须同步，手动可重试；自动刷新不重复同步。
  await page.evaluate(()=>{snapshot.holdings[1].nav.date='2026-09-30';Fund.actions.loadPositions();});
  await page.waitForFunction(()=>calls.includes('/api/nav/sync') && replies.at(-1).url.includes('refresh=1'));
  const syncCount=await page.evaluate(()=>calls.filter(c=>c==='/api/nav/sync').length);
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  await page.evaluate(()=>{Fund.actions.refreshPositionsQuotes();});
  await page.waitForFunction(()=>replies.at(-1).url.includes('refresh=1'));
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  assert.equal(await page.evaluate(()=>calls.filter(c=>c==='/api/nav/sync').length),syncCount);
  await page.evaluate(()=>{window.navError=true;Fund.actions.refreshPositionsQuotes(true);});
  await page.waitForFunction(()=>calls.filter(c=>c==='/api/nav/sync').length>1);
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  await page.waitForFunction(()=>document.getElementById('positions-status').textContent.includes('NAV sync failed'));
  await page.evaluate(()=>{window.navError=false;window.navBlocked=true;Fund.actions.refreshPositionsQuotes(true);});
  await page.waitForFunction(()=>!!window.navReply);
  const requestsDuringSync=await page.evaluate(()=>calls.length);
  await page.evaluate(()=>{Fund.actions.refreshPositionsQuotes();Fund.actions.leavePositions();});
  assert.equal(await page.evaluate(()=>calls.length),requestsDuringSync,'shared tick cannot overlap an active NAV sync');
  assert.equal(await page.evaluate(()=>navReply.signal.aborted),true);
  await page.evaluate(()=>{window.navBlocked=false;navReply.reject(Object.assign(new Error('cancelled'),{name:'AbortError'}));});
  await page.evaluate(()=>{Fund.actions.refreshPositionsQuotes(true);});
  await page.waitForFunction(()=>calls.filter(c=>c==='/api/nav/sync').length>3);
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  const missing=await page.evaluate(()=>Fund.positionIncome.inline({income:{day:{date:'2026-10-09',amount:null,rateBps:null,reason:'missing_daily_price'}},dayPnl:'999',dayPct:20,nav:{date:'2026-09-30'}},'day'));
  assert.doesNotMatch(missing,/999|20.00/,'legacy dayPnl must not resurrect stale returns');
  assert.match(missing,/10.*9/);
  await page.locator('.pos-card-details > summary').first().press('Enter');
  const opened=await page.locator('.pos-card-details[open]').count();
  await page.evaluate(()=>{Fund.actions.refreshPositionsQuotes();});
  await page.evaluate(()=>replies.at(-1).resolve(structuredClone(snapshot)));
  await page.waitForFunction(()=>document.querySelector('.pos-card-details').open);
  assert.equal(await page.locator('.pos-card-details[open]').count(),opened,'refresh keeps card details open');
  assert.deepEqual(errors, []);
  console.log('positions Chrome synthetic API: race / cancel / exact value / privacy / i18n / XSS / keyboard / single AI scope passed');
} finally { await browser.close(); }
