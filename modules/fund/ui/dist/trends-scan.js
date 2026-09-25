(() => {
const { authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});

// ===== 全市场扫描功能模块 (Trends Scan) =====
// 职责：负责全A股双周期共振扫描弹窗、进度轮询、结果呈现与一键跳转分析
let _scanTimer = null;

function openScan() {
  const overlay = document.getElementById('scan-overlay');
  if (overlay) overlay.classList.add('show');
  authFetch('/api/trends/scan').then(r => r.json()).then(data => {
    if (data.status === 'running') {
      renderScanProgress(data);
      startScanPolling();
    } else if (data.status === 'done' && data.results && data.results.length > 0) {
      renderScanResults(data);
    } else {
      renderScanIdle();
    }
  }).catch(() => { renderScanIdle(); });
}

function closeScan(e) {
  if (e && e.target !== document.getElementById('scan-overlay')) return;
  const overlay = document.getElementById('scan-overlay');
  if (overlay) overlay.classList.remove('show');
  stopScanPolling();
}

function renderScanIdle() {
  const el = document.getElementById('scan-content');
  if (!el) return;
  el.innerHTML = `
    <div class="scan-empty">
      <div style="margin-bottom:16px;font-size:15px;color:#aaa">扫描全A股，找出日K和周K同时符合买入信号的股票</div>
      <div style="margin-bottom:8px;color:#666;font-size:13px">扫描范围：成交额前1000只活跃A股</div>
      <div style="margin-bottom:8px;color:#666;font-size:13px">筛选条件：日K买入 + 周K买入（双周期共振）</div>
      <div style="margin-bottom:20px;color:#666;font-size:13px">预计耗时：2-4分钟</div>
      <button class="scan-btn" style="font-size:15px;padding:8px 28px" onclick="startScan()">开始扫描</button>
    </div>`;
}

function startScan() {
  const el = document.getElementById('scan-content');
  if (el) {
    el.innerHTML = `
      <div class="scan-progress-wrap">
        <div class="scan-stage">正在启动扫描...</div>
        <div class="scan-bar-bg"><div class="scan-bar-fill" style="width:0%"></div></div>
      </div>`;
  }
  authFetch('/api/trends/scan?action=start').then(r => r.json()).then(data => {
    if (data.status === 'started' || data.status === 'running') {
      startScanPolling();
    }
  });
}

function startScanPolling() {
  stopScanPolling();
  _scanTimer = setInterval(() => {
    authFetch('/api/trends/scan').then(r => r.json()).then(data => {
      if (data.status === 'running') {
        renderScanProgress(data);
      } else if (data.status === 'done') {
        stopScanPolling();
        renderScanResults(data);
      } else if (data.status === 'error') {
        stopScanPolling();
        renderScanError(data);
      }
    }).catch(() => {});
  }, 2000);
}

function stopScanPolling() {
  if (_scanTimer) { clearInterval(_scanTimer); _scanTimer = null; }
}

function renderScanProgress(data) {
  const el = document.getElementById('scan-content');
  if (!el) return;
  const pct = data.progress || 0;
  const stage = data.stage || '扫描中...';
  const scanned = data.scanned || 0;
  const total = data.total || 0;
  const found = data.found || 0;
  const elapsed = data.elapsed || 0;
  el.innerHTML = `
    <div class="scan-progress-wrap">
      <div class="scan-stage">${stage}</div>
      <div class="scan-bar-bg"><div class="scan-bar-fill" style="width:${pct}%"></div></div>
      <div class="scan-stats">
        <span>进度: <b>${scanned}/${total}</b></span>
        <span>发现买入: <b style="color:#cdf24b">${found}</b></span>
        <span>耗时: <b>${elapsed}s</b></span>
        <span>进度: <b>${pct}%</b></span>
      </div>
    </div>
    <div class="scan-empty">正在扫描中，请耐心等待...</div>`;
}

function renderScanResults(data) {
  const el = document.getElementById('scan-content');
  if (!el) return;
  const results = data.results || [];
  const elapsed = data.elapsed || 0;
  if (!results.length) {
    el.innerHTML = `
      <div class="scan-empty">
        <div style="margin-bottom:12px;color:#aaa">扫描完成，未发现双周期买入信号</div>
        <div style="color:#666;font-size:13px">当前市场可能处于调整期，可稍后再试</div>
        <div style="margin-top:16px"><button class="scan-btn" onclick="renderScanIdle()">重新扫描</button></div>
      </div>`;
    return;
  }
  let html = `
    <div class="scan-stats" style="margin-bottom:12px">
      <span>扫描完成，耗时 <b style="color:#ddd">${elapsed}s</b></span>
      <span>双周期买入: <b style="color:#cdf24b">${results.length}</b> 只</span>
      <button class="scan-btn" style="margin-left:auto;padding:3px 12px;font-size:12px" onclick="renderScanIdle()">重新扫描</button>
    </div>
    <table class="scan-table">
      <thead><tr>
        <th>#</th><th>代码</th><th>名称</th><th>现价</th>
        <th>日K信号</th><th>日K分</th>
        <th>周K信号</th><th>周K分</th>
        <th>综合分</th><th>仓位</th><th>盈亏比</th>
        <th>操作</th>
      </tr></thead>
      <tbody>`;
  results.forEach((r, i) => {
    const dAct = formatScanAction(r.daily_action);
    const wAct = formatScanAction(r.weekly_action);
    const pct = (r.daily_pct || 0).toFixed(2);
    const pctColor = r.daily_pct > 0 ? '#ff2d2d' : r.daily_pct < 0 ? '#00b35c' : '#888';
    html += `<tr>
      <td class="scan-rank">${i + 1}</td>
      <td>${r.symbol}</td>
      <td>${r.name}</td>
      <td style="color:${pctColor}">${r.price ? r.price.toFixed(2) : '-'}<span style="font-size:11px;color:#666"> ${pct}%</span></td>
      <td class="${dAct.cls}">${dAct.text}</td>
      <td>${r.daily_score}</td>
      <td class="${wAct.cls}">${wAct.text}</td>
      <td>${r.weekly_score}</td>
      <td class="scan-combined" style="color:#cdf24b">${r.combined_score}</td>
      <td style="font-size:12px;color:#aaa">${r.position_advice ? r.position_advice.split('—')[0].trim() : '-'}</td>
      <td style="color:${(r.risk_reward||0) >= 2 ? '#00b35c' : (r.risk_reward||0) >= 1 ? '#ffc107' : '#ff2d2d'}">${r.risk_reward || '-'}</td>
      <td><button class="scan-analyze-btn" onclick="analyzeFromScan('${r.symbol}')">分析</button></td>
    </tr>`;
  });
  html += '</tbody></table>';
  el.innerHTML = html;
}

function formatScanAction(act) {
  if (!act) return { text: '-', cls: 'scan-action-watch' };
  if (act.includes('强烈')) return { text: '强买', cls: 'scan-action-strong' };
  if (act.includes('买入') && !act.includes('谨慎')) return { text: '买入', cls: 'scan-action-buy' };
  if (act.includes('谨慎')) return { text: '谨慎', cls: 'scan-action-caution' };
  if (act.includes('卖出')) return { text: '卖出', cls: 'scan-action-watch' };
  return { text: '观望', cls: 'scan-action-watch' };
}

function analyzeFromScan(symbol) {
  closeScan();
  if (trends.coordinator?.analyze) {
    trends.coordinator.analyze(symbol);
  }
}

function renderScanError(data) {
  const el = document.getElementById('scan-content');
  if (!el) return;
  el.innerHTML = `
    <div class="scan-empty">
      <div style="margin-bottom:12px;color:#ff4d4d">扫描失败</div>
      <div style="color:#888;font-size:13px">${data.error || '未知错误'}</div>
      <div style="margin-top:16px"><button class="scan-btn" onclick="renderScanIdle()">重试</button></div>
    </div>`;
}

// 绑定全局与内联事件
window.openScan = openScan;
window.closeScan = closeScan;
window.startScan = startScan;
window.renderScanIdle = renderScanIdle;
window.analyzeFromScan = analyzeFromScan;

const scanBtn = document.getElementById('scan-btn');
if (scanBtn) scanBtn.addEventListener('click', openScan);
const scanCloseBtn = document.getElementById('scan-close-btn');
if (scanCloseBtn) scanCloseBtn.addEventListener('click', () => closeScan());
const scanOverlay = document.getElementById('scan-overlay');
if (scanOverlay) scanOverlay.addEventListener('click', (e) => closeScan(e));

trends.scan = {
  openScan,
  closeScan,
  startScan,
  renderScanIdle,
  analyzeFromScan,
};

})();
