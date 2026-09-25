(() => {
const { state, actions, authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const charts = trends.charts;
const C = trends.C;

// 资金流图表与实时流动模块已拆分至 trends-flow.js
const flowMod = () => trends.flow || {};
function renderFlow(...args) { return flowMod().renderFlow?.(...args); }
function switchFlowMode(...args) { return flowMod().switchFlowMode?.(...args); }
function loadRealtimeFlow(...args) { return flowMod().loadRealtimeFlow?.(...args); }
function renderRealtimeFlow(...args) { return flowMod().renderRealtimeFlow?.(...args); }

// ===== 一句话总结（小白第一眼看的） =====
function renderSummary(signal) {
  const el = document.getElementById('sum-body');
  if (!el) return;
  const action = signal.action;
  const isBuy = action.includes('买入') && action !== '谨慎买入';
  const isCautious = action === '谨慎买入';
  const isSell = action.includes('卖出');
  const badgeClass = isBuy ? 'sum-badge-buy' : isCautious ? 'sum-badge-cautious' : isSell ? 'sum-badge-sell' : 'sum-badge-watch';
  const badgeText = action;

  const strength = signal.signal_strength || '';
  const sClass = strength === '强' ? 'strong' : strength === '中' ? 'medium' : 'weak';
  const sText = strength ? `信号${strength}` : '';

  const risk = signal.risk_level || '';
  const riskClass = risk === '低' ? 'low' : risk === '中' ? 'mid' : 'high';

  const score = signal.score || 0;
  const scoreColor = isBuy ? C.up : isSell ? C.down : '#ffc107';

  let summary = signal.plain_summary || '';

  const origAction = signal.original_action || '';
  const vetoReason = signal.veto_reason || '';
  const riskNotes = signal.risk_notes || [];
  const posAdvice = signal.position_advice || '';
  const riskReward = signal.risk_reward || 0;

  let optimizeHtml = '';
  if (origAction && origAction !== action) {
    optimizeHtml = `<div style="margin-top:4px;font-size:11px;color:#888">
      <span style="text-decoration:line-through;color:#666">${origAction}</span>
      <span style="margin:0 2px">→</span>
      <span style="color:${isCautious ? '#ffb74d' : '#ffc107'};font-weight:500">${action}</span>
      ${vetoReason ? `<span style="margin-left:6px;color:#ff6b6b">⚠ ${vetoReason}</span>` : ''}
    </div>`;
  } else if (vetoReason) {
    optimizeHtml = `<div style="margin-top:4px;font-size:11px;color:#ff6b6b">⚠ ${vetoReason}</div>`;
  }

  let riskNotesHtml = '';
  if (riskNotes.length > 0) {
    riskNotesHtml = `<div style="margin-top:6px;padding:6px 8px;background:rgba(255,152,0,0.08);border-radius:6px;border:1px solid rgba(255,152,0,0.15)">
      ${riskNotes.map(n => `<div style="font-size:11px;color:#ffb74d;line-height:1.5">⚠ ${n}</div>`).join('')}
    </div>`;
  }

  let posHtml = '';
  if (posAdvice && posAdvice !== '空仓等待' && (isBuy || isCautious)) {
    posHtml = `<div style="margin-top:4px;font-size:11px;color:#aaa">
      <span style="color:#cdf24b">建议仓位：</span><span style="color:#ddd">${posAdvice}</span>
      ${riskReward ? `<span style="margin-left:8px;color:#888">盈亏比 ${riskReward}</span>` : ''}
    </div>`;
  }

  el.innerHTML = `
    <div class="sum-action-row">
      <span class="sum-badge ${badgeClass}">${badgeText}</span>
      ${sText ? `<span class="sum-strength ${sClass}">${sText}</span>` : ''}
      <span class="sum-score-big" style="color:${scoreColor}">${score}分</span>
    </div>
    <div class="sum-text">${summary}</div>
    ${optimizeHtml}
    ${posHtml}
    ${riskNotesHtml}
    <div class="sum-risk-row">
      <span class="sum-risk-dot ${riskClass}"></span>
      <span style="color:#888">风险等级：<span style="color:${risk==='低'?C.down:risk==='高'?C.up:'#ffc107'};font-weight:bold">${risk}</span></span>
      <span style="color:#555;margin-left:auto">置信度 ${signal.confidence||0}%</span>
    </div>
  `;
}

// ===== 操作计划 =====
function renderTradePlan(signal) {
  const card = document.getElementById('plan-card');
  const el = document.getElementById('plan-body');
  if (!card || !el) return;
  const plan = signal.trade_plan;
  if (!plan || !plan.action) { card.style.display = 'none'; return; }
  card.style.display = 'block';

  const action = signal.action || plan.action;
  const isBuy = action.includes('买入') && action !== '谨慎买入';
  const isCautious = action === '谨慎买入';
  const isSell = action.includes('卖出');
  const isWatch = action === '观望';

  const entry = plan.entry_price || 0;
  const stop = plan.stop_loss || 0;
  const target = plan.target_price || 0;
  const rr = plan.risk_reward_ratio || 0;
  const lossPct = plan.max_loss_pct || 0;
  const pos = plan.position_size || '';
  const period = plan.holding_period || '';
  const notes = plan.notes || '';

  if (isWatch) {
    const vetoReason = signal.veto_reason || '';
    const vetoHtml = vetoReason ? `
      <div style="padding:6px 8px;margin-bottom:6px;background:rgba(255,107,107,0.08);border-radius:6px;border:1px solid rgba(255,107,107,0.15);font-size:11px;color:#ff6b6b;line-height:1.5">
        ⚠ ${vetoReason}
      </div>` : '';
    el.innerHTML = `
      ${vetoHtml}
      <div style="padding:8px 0;font-size:13px;color:#aaa;line-height:1.6">
        <div style="margin-bottom:6px"><span style="color:#ffc107">当前建议：</span>${pos || signal.position_advice || '空仓等待'}</div>
        <div style="margin-bottom:6px"><span style="color:#888">适合周期：</span>${period}</div>
        <div class="plan-notes">${notes}</div>
      </div>`;
    return;
  }

  if (isSell) {
    el.innerHTML = `
      <div style="padding:8px 0;font-size:13px;color:#aaa;line-height:1.6">
        <div style="margin-bottom:6px"><span style="color:${C.down}">操作建议：</span>${pos}</div>
        <div class="plan-notes">${notes}</div>
      </div>`;
    return;
  }

  const cautionBanner = isCautious ? `
    <div style="padding:6px 8px;margin-bottom:8px;background:rgba(255,152,0,0.1);border-radius:6px;border:1px solid rgba(255,152,0,0.2);font-size:11px;color:#ffb74d;line-height:1.5">
      ⚠ 谨慎买入：信号存在风险因素，建议轻仓试探，严格执行止损
    </div>` : '';
  el.innerHTML = `
    ${cautionBanner}
    <div class="plan-prices">
      <div class="plan-price-box">
        <div class="plan-price-label">买入价 <span class="plan-tip">现价入手</span></div>
        <div class="plan-price-val" style="color:${C.up}">${entry.toFixed(2)}</div>
      </div>
      <div class="plan-price-box">
        <div class="plan-price-label">止损价 <span class="plan-tip">跌到这里就卖</span></div>
        <div class="plan-price-val" style="color:${C.down}">${stop.toFixed(2)}</div>
      </div>
      <div class="plan-price-box">
        <div class="plan-price-label">目标价 <span class="plan-tip">涨到这里就卖</span></div>
        <div class="plan-price-val" style="color:#cdf24b">${target.toFixed(2)}</div>
      </div>
    </div>
    <div class="plan-rr">
      <span class="plan-rr-item">盈亏比 <b>${rr || signal.risk_reward || 0}</b> <span class="plan-tip">冒1元风险可赚${rr || signal.risk_reward || 0}元</span></span>
      <span class="plan-rr-item">最大亏损 <b style="color:${C.down}">${lossPct}%</b></span>
    </div>
    <div class="plan-row">
      <span class="plan-label">建议仓位 <span class="plan-tip">投多少钱</span></span>
      <span class="plan-val" style="color:#cdf24b">${pos || signal.position_advice || ''}</span>
    </div>
    <div class="plan-row">
      <span class="plan-label">持有周期 <span class="plan-tip">大概持多久</span></span>
      <span class="plan-val">${period}</span>
    </div>
    <div class="plan-notes">${notes}</div>
  `;
}

// ===== 信号面板 =====
function renderSignal(signal) {
  renderSummary(signal);
  renderTradePlan(signal);

  const ml = document.getElementById('module-list');
  if (ml) {
    const ms = signal.module_scores || {};
    const labels = { '趋势': '趋势方向', '形态': '图表形态', '量价': '量价关系', '突破': '突破信号', 'CAN_SLIM': '综合基本面' };
    const sorted = Object.entries(ms).sort((a, b) => b[1] - a[1]);
    ml.innerHTML = sorted.map(([k, v]) => {
      const pct = Math.min(100, Math.max(0, v));
      const c = pct >= 65 ? C.up : pct >= 45 ? '#ffc107' : C.down;
      const tag = pct >= 65 ? '偏多' : pct >= 45 ? '中性' : '偏空';
      return `<div class="ml-row">
        <span class="ml-name">${labels[k] || k}</span>
        <div class="ml-bar-bg"><div class="ml-bar" style="width:${pct}%;background:${c}"></div></div>
        <span class="ml-val" style="color:${c}">${v}</span>
        <span style="font-size:10px;color:${c};width:28px">${tag}</span>
      </div>`;
    }).join('');
  }

  const sl = document.getElementById('sig-list');
  if (sl) {
    const buys = (signal.buy_signals || []).map(s => ({ type: 'buy', text: s }));
    const sells = (signal.sell_signals || []).map(s => ({ type: 'sell', text: s }));
    const allSigs = [...buys, ...sells].sort((a, b) => {
      const weight = (t) => /强势|确认|突破|流入|优秀/.test(t) ? 0 : 1;
      return weight(a.text) - weight(b.text);
    });
    sl.innerHTML = allSigs.map((s, idx) => {
      const isCore = idx < 2 && allSigs.length > 2;
      const coreTag = isCore ? '<span class="sig-core-tag">核心</span>' : '';
      if (s.type === 'buy') return `<div class="sig-item sig-buy">▲ ${s.text}${coreTag}</div>`;
      else return `<div class="sig-item sig-sell">▼ ${s.text}${coreTag}</div>`;
    }).join('') || '<div style="color:#555;font-size:12px;padding:8px">暂无信号</div>';
  }

  const rc = document.getElementById('risk-card');
  const rl = document.getElementById('risk-list');
  if (rc && rl) {
    const risks = signal.risk_warnings || [];
    if (risks.length) {
      rc.style.display = 'block';
      rl.innerHTML = risks.map(w => `<div class="sig-item sig-risk">⚠ ${w}</div>`).join('');
    } else { rc.style.display = 'none'; }
  }
}

// ===== CANSLIM =====
function renderCanslim(cs) {
  if (!cs) return;
  const eh = document.getElementById('cs-empty-hint');
  if (eh) eh.style.display = 'none';
  const items = [
    { l: 'C', s: cs.c_score, n: '近期动力', tip: '近期价格涨势' },
    { l: 'A', s: cs.a_score, n: '中期趋势', tip: '中期价格趋势' },
    { l: 'N', s: cs.n_score, n: '新高形态', tip: '是否创新高/新底部' },
    { l: 'S', s: cs.s_score, n: '供需关系', tip: '换手率/量价配合' },
    { l: 'L', s: cs.l_score, n: '领涨强度', tip: '相对大盘强度' },
    { l: 'I', s: cs.i_score, n: '机构资金', tip: '主力资金流向' },
    { l: 'M', s: cs.m_score, n: '大盘环境', tip: '市场整体方向' },
  ];
  const grid = document.getElementById('cs-grid');
  if (grid) {
    grid.innerHTML = items.map(i => {
      const c = i.s >= 65 ? C.up : i.s >= 45 ? '#ffc107' : C.down;
      const tag = i.s >= 65 ? '好' : i.s >= 45 ? '中' : '差';
      return `<div class="cs-item" title="${i.tip}">
        <span class="cs-letter">${i.l}</span>
        <span class="cs-score" style="color:${c}">${i.s}</span>
        <span class="cs-label">${i.n}</span>
        <span style="font-size:9px;color:${c}">${tag}</span>
      </div>`;
    }).join('');
  }

  const simple = document.getElementById('cs-simple');
  if (simple) {
    const totalScore = Math.round((cs.c_score + cs.a_score + cs.n_score + cs.s_score + cs.l_score + cs.i_score + cs.m_score) / 7);
    const tColor = totalScore >= 65 ? C.up : totalScore >= 45 ? '#ffc107' : C.down;
    const tLabel = totalScore >= 65 ? '良好' : totalScore >= 45 ? '一般' : '较差';
    const sorted = [...items].sort((a, b) => b.s - a.s);
    const best = sorted[0], worst = sorted[sorted.length - 1];
    simple.innerHTML = `
      <div style="display:flex;align-items:center;gap:8px">
        <span class="cs-simple-score" style="color:${tColor}">${totalScore}</span>
        <span class="cs-simple-label">基本面综合评分（${tLabel}）</span>
      </div>
      <div style="font-size:11px;color:#888;margin-top:6px">
        最强：<span style="color:${best.s >= 65 ? C.up : '#ffc107'}">${best.n} ${best.s}分</span>　
        最弱：<span style="color:${worst.s >= 45 ? '#ffc107' : C.down}">${worst.n} ${worst.s}分</span>
      </div>
    `;
  }
}

// ===== 关键价位 =====
const KL_TOOLTIPS = {
  '趋势线': '连接近期重要高点或低点的直线，价格触及此处可能反弹或突破。',
  '颈线': '头肩/双顶/双底等形态的颈线位，突破后视为形态确认。',
  '头部': '头肩形态中的极值点，是形态测量的基准价位。',
  '止损': '海龟交易法则的2N止损位。跌破（做多）或涨破（做空）此处应离场止损。',
  '系统一': '海龟法则20日唐奇安通道触发，短期突破系统。',
  '系统二': '海龟法则55日唐奇安通道触发，中长期突破系统。',
  '支撑': '价格下跌时可能获得买盘支撑的位置。',
  '压力': '价格上涨时可能遭遇卖盘压力的位置。',
  '压力位': '价格上涨时可能遭遇卖盘压力的位置。',
};

function explainKeyLevel(label) {
  for (const [key, text] of Object.entries(KL_TOOLTIPS)) {
    if (label.includes(key)) return text;
  }
  return '鼠标悬浮查看该价位含义；关键价位来自趋势线、形态颈线或海龟交易系统。';
}

function renderKeyLevels(levels) {
  const el = document.getElementById('kl-grid');
  if (!el) return;
  const eh = document.getElementById('kl-empty-hint');
  if (eh) eh.style.display = 'none';
  if (!levels || !Object.keys(levels).length) {
    el.innerHTML = '<span style="color:#555;font-size:12px;padding:8px">无关键价位</span>';
    return;
  }
  el.innerHTML = Object.entries(levels).map(([k, v]) => {
    const tip = explainKeyLevel(k);
    return `<div class="kl-item" title="${tip}" style="cursor:help">
      <span class="kl-label" style="border-bottom:1px dashed #444">${k}</span>
      <span class="kl-val">${v.toFixed(2)}</span>
    </div>`;
  }).join('');
}

// ===== 大盘环境 =====
function renderMarket(signal, breadth) {
  const card = document.getElementById('market-card');
  const el = document.getElementById('market-body');
  if (!card || !el) return;
  if (!signal.canslim) { card.style.display = 'none'; return; }

  const m = signal.canslim.m_score;
  const isBearish = m < 40;

  card.style.display = 'block';
  const mColor = m >= 65 ? C.up : m >= 45 ? '#ffc107' : C.down;
  const mLabel = m >= 65 ? '偏多' : m >= 45 ? '中性' : '偏空';

  let breadthHtml = '';
  if (breadth && breadth.total >= 50) {
    const upN = breadth.up || 0;
    const downN = breadth.down || 0;
    const br = breadth.breadth_ratio || 0.5;
    const brPct = (br * 100).toFixed(0);
    const brColor = br >= 0.6 ? C.up : br >= 0.4 ? '#ffc107' : C.down;
    const brLabel = br >= 0.7 ? '普涨' : br >= 0.6 ? '多数上涨' : br >= 0.4 ? '多数下跌' : '普跌';
    breadthHtml = `<div style="margin-top:6px;padding:6px 8px;background:rgba(255,255,255,0.05);border-radius:6px">
      <div style="display:flex;align-items:center;gap:8px;font-size:12px">
        <span style="color:${C.up};font-weight:bold">${upN}</span>
        <span style="color:#666">涨</span>
        <span style="color:#444">/</span>
        <span style="color:${C.down};font-weight:bold">${downN}</span>
        <span style="color:#666">跌</span>
        <span style="color:${brColor};font-weight:bold;margin-left:auto">${brPct}% ${brLabel}</span>
      </div>
    </div>`;
  }

  let advice = '';
  if (isBearish && breadth && breadth.breadth_ratio >= 0.55) {
    advice = '<div style="color:#ffc107;margin-top:4px;font-size:11px">大盘趋势偏空，但今日多数个股上涨，短线可关注反弹</div>';
  } else if (isBearish) {
    advice = `<div style="color:#ff2d2d;margin-top:4px;font-size:11px">大盘偏空${breadth ? `，今日${breadth.up}涨/${breadth.down}跌` : ''}，建议降低仓位或等待大盘转暖</div>`;
  } else if (m >= 65) {
    advice = '<div style="color:#00b35c;margin-top:4px;font-size:11px">大盘环境偏多，适合积极操作</div>';
  } else {
    advice = '<div style="color:#ffc107;margin-top:4px;font-size:11px">大盘环境中性，可适度操作但需谨慎</div>';
  }

  const mSignals = (signal.canslim.signals || []).filter(s =>
    s.includes('市场环境') || s.includes('大盘') || s.includes('M(') || s.includes('今日')
  );
  const mSignalText = mSignals.join('；');

  el.innerHTML = `
    <div style="display:flex;align-items:center;gap:10px;margin-bottom:6px">
      <span style="font-size:14px;font-weight:bold;color:${mColor}">${mLabel}</span>
      <span style="font-size:28px;font-weight:bold;color:${mColor};line-height:1">${m}分</span>
      <span style="font-size:11px;color:#666;margin-left:auto">上证指数环境</span>
    </div>
    ${mSignalText ? `<div style="color:#888;font-size:11px">${mSignalText}</div>` : ''}
    ${breadthHtml}
    ${advice}
  `;
}

// ===== 小白/专业模式切换 =====
function loadMode() {
  try { tState.mode = localStorage.getItem('qs_mode') || 'pro'; } catch(e) { tState.mode = 'pro'; }
  applyMode();
}

function setMode(mode) {
  tState.mode = mode;
  try { localStorage.setItem('qs_mode', mode); } catch(e) {}
  applyMode();
  if (mode === 'simple') {
    collapseCard('canslim', true);
    collapseCard('levels', true);
    collapseCard('chanlun-daily', true);
    collapseCard('chanlun-minute', true);
    collapseCard('accuracy', true);
    collapseCard('risk', true);
  } else {
    document.querySelectorAll('.signal-card.collapsed, .chanlun-card.collapsed').forEach(c => c.classList.remove('collapsed'));
  }
}

function applyMode() {
  document.body.classList.remove('mode-pro', 'mode-simple');
  document.body.classList.add('mode-' + tState.mode);
  const mtPro = document.getElementById('mt-pro');
  if (mtPro) mtPro.classList.toggle('active', tState.mode === 'pro');
  const mtSimple = document.getElementById('mt-simple');
  if (mtSimple) mtSimple.classList.toggle('active', tState.mode === 'simple');
}

// ===== 卡片折叠 =====
function toggleCard(headerEl) {
  const card = headerEl.closest('.signal-card, .chanlun-card, .side-card, .summary-card, .plan-card');
  if (card) card.classList.toggle('collapsed');
}

function collapseCard(cardName, collapse) {
  const card = document.querySelector(`[data-card="${cardName}"]`);
  if (card) card.classList.toggle('collapsed', collapse);
}

// 绑定模式与资金流切换
const mtPro = document.getElementById('mt-pro');
if (mtPro) mtPro.onclick = () => setMode('pro');
const mtSimple = document.getElementById('mt-simple');
if (mtSimple) mtSimple.onclick = () => setMode('simple');

window.toggleCard = toggleCard;
window.setMode = setMode;
window.switchFlowMode = switchFlowMode;

trends.panel = {
  renderFlow,
  switchFlowMode,
  loadRealtimeFlow,
  renderRealtimeFlow,
  renderSummary,
  renderTradePlan,
  renderSignal,
  renderCanslim,
  renderKeyLevels,
  explainKeyLevel,
  renderMarket,
  loadMode,
  setMode,
  applyMode,
  toggleCard,
  collapseCard,
};

})();
