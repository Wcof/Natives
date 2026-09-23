// Token Monitor Authentic Frontend Controller
(function () {
  'use strict';

  let token = '';
  let activePeriod = 'today';
  let activeView = 'home';
  let overviewData = null;
  let toolsData = [];
  let limitsData = [];
  let sessionsData = [];
  let modelsData = [];
  let trendsData = [];

  // Authentic Brand Colors from Token Monitor
  const CLIENT_COLORS = {
    claude: '#cc7c5e',
    codex: '#49a3b0',
    opencode: '#3b82f6',
    hermes: '#d4af37',
    openclaw: '#ff4d4d',
    cursor: '#00b4d8',
    antigravity: '#4285f4',
    cline: '#323b43',
    amp: '#f34e3f',
    droid: '#10b981',
    kimi: '#6366f1',
    qwen: '#615ced',
    grok: '#f43f5e',
    copilot: '#10a37f',
    pi: '#8b5cf6',
    zed: '#4173e7',
    kilo: '#f8f676',
    commandcode: '#8c4edd',
    zcode: '#ec4899',
    kiro: '#9046ff',
    codebuddy: '#6c4dff',
    workbuddy: '#0dc8a5',
    proma: '#64748b',
    qodercn: '#2adb5c',
    reasonix: '#4d6bfe',
    dsh: '#4d6bfe',
    cherrystudio: '#ea5e5d',
    lmstudio: '#6c5ce7',
    unsloth: '#40b85a',
    openrouter: '#6566f1',
    gemini: '#4285f4',
    deepseek: '#4d6bfe',
    minimax: '#f23f5d',
    volcengine: '#006eff',
    alibaba: '#615ced',
    default: '#6ab4f0'
  };

  function getClientColor(id) {
    return CLIENT_COLORS[id] || CLIENT_COLORS.default;
  }

  // 接收宿主页（app.html）双阶段握手消息
  window.addEventListener('message', async (event) => {
    const data = event.data;
    if (!data) return;

    if (data.type === 'init') {
      window.parent.postMessage({
        type: 'hello',
        generation: data.generation,
        challenge: data.challenge
      }, '*');
    } else if (data.type === 'welcome') {
      token = data.token;
      initApp();
    } else if (data.type === 'theme') {
      document.documentElement.dataset.theme = data.appearance === 'dark' ? 'dark' : 'light';
    }
  });

  async function apiCall(route, options = {}) {
    const headers = {
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json',
      ...(options.headers || {})
    };
    const res = await fetch(route, { ...options, headers });
    if (!res.ok) throw new Error(`API error: ${res.status}`);
    return res.json();
  }

  function formatNumber(n) {
    return (Number(n) || 0).toLocaleString();
  }

  function formatCompactTokens(n) {
    n = Number(n) || 0;
    if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B';
    if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
    if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K';
    return n.toString();
  }

  function formatMoney(amount) {
    const n = Number(amount) || 0;
    return '$' + n.toFixed(2);
  }

  function escapeHtml(str) {
    return String(str || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  async function initApp() {
    setupEventListeners();
    await refreshAll();
  }

  function setupEventListeners() {
    // Period Tabs (DAY, WEEK, MONTH, TOTAL)
    document.querySelectorAll('.period-tab').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.period-tab').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        activePeriod = btn.dataset.period || 'today';
        updateTotalPanel();
        if (activeView === 'home') renderHomeBreakdown();
      });
    });

    // View Switcher (Home, Limits, Tools, Sessions, Models, Trends)
    document.querySelectorAll('.view-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.view-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        activeView = btn.dataset.view || 'home';
        switchView(activeView);
      });
    });

    // Refresh Button
    document.getElementById('btn-refresh')?.addEventListener('click', async () => {
      const btn = document.getElementById('btn-refresh');
      btn.disabled = true;
      try {
        await apiCall('/api/collect', { method: 'POST' });
        await refreshAll();
      } catch (err) {
        console.error('Refresh failed:', err);
      } finally {
        btn.disabled = false;
      }
    });

    // Session Modal Close
    document.getElementById('btn-modal-close')?.addEventListener('click', () => {
      document.getElementById('session-modal').style.display = 'none';
    });
    document.getElementById('session-modal')?.addEventListener('click', (e) => {
      if (e.target.id === 'session-modal') {
        document.getElementById('session-modal').style.display = 'none';
      }
    });

    // Sessions Table Row Click Delegation
    document.getElementById('sessions-table-body')?.addEventListener('click', (e) => {
      const row = e.target.closest('.session-row');
      if (row && row.dataset.sessionId) {
        openSessionModal(row.dataset.sessionId);
      }
    });
  }

  function switchView(view) {
    const sections = ['home', 'limits', 'tools', 'sessions', 'models', 'trends'];
    for (const s of sections) {
      const el = document.getElementById(`view-${s}`);
      if (el) el.style.display = s === view ? (s === 'limits' ? 'grid' : 'block') : 'none';
    }
  }

  async function refreshAll() {
    try {
      const [overview, tools, limits, sessions, models, trends] = await Promise.all([
        apiCall('/api/overview'),
        apiCall('/api/tools'),
        apiCall('/api/limits'),
        apiCall('/api/sessions'),
        apiCall('/api/models'),
        apiCall('/api/trends')
      ]);

      overviewData = overview;
      toolsData = tools.tools || [];
      limitsData = limits.limits || [];
      sessionsData = sessions.sessions || [];
      modelsData = models.models || [];
      trendsData = trends.trends || [];

      updateTotalPanel();
      renderHomeBreakdown();
      renderLimitsGrid();
      renderToolsTable();
      renderSessionsTable();
      renderModelsTable();
      renderTrendsTable();
    } catch (err) {
      console.error('refreshAll failed:', err);
    }
  }

  function updateTotalPanel() {
    if (!overviewData) return;
    const periods = overviewData.periods || {};
    let currentPeriod = periods.today || {};
    if (activePeriod === 'week') currentPeriod = periods.thisWeek || currentPeriod;
    else if (activePeriod === 'month') currentPeriod = periods.thisMonth || currentPeriod;
    else if (activePeriod === 'allTime') currentPeriod = periods.allTime || currentPeriod;

    const totalEl = document.getElementById('totalTokens');
    const costEl = document.getElementById('cost');

    if (totalEl) totalEl.textContent = formatNumber(currentPeriod.totalTokens || 0);
    if (costEl) costEl.textContent = formatMoney(currentPeriod.costUsd || 0);
  }

  // 1. Home View Breakdown (Signature Token Monitor Style)
  function renderHomeBreakdown() {
    const container = document.getElementById('view-home');
    if (!container) return;

    if (!toolsData || toolsData.length === 0) {
      container.innerHTML = '<div style="text-align:center; padding:30px; color:var(--muted);">暂无活跃工具用量记录</div>';
      return;
    }

    const maxTokens = Math.max(...toolsData.map(t => t.totalTokens || 0), 1);

    container.innerHTML = toolsData.map(t => {
      const color = getClientColor(t.id);
      const percent = Math.min(100, Math.max(2, Math.round((t.totalTokens / maxTokens) * 100)));

      return `
        <div class="tool-row">
          <div class="tool-row-head">
            <div class="tool-brand">
              <span class="tool-dot" style="background:${color}; box-shadow:0 0 6px ${color}88;"></span>
              <span class="tool-name">${escapeHtml(t.name)}</span>
            </div>
            <div class="tool-metrics">
              <span class="tool-tokens">${formatCompactTokens(t.totalTokens)}</span>
              <span class="tool-cost">${formatMoney(t.costUsd)}</span>
            </div>
          </div>
          <div class="progress-track">
            <div class="progress-fill" style="width:${percent}%; background:${color};"></div>
          </div>
        </div>
      `;
    }).join('');
  }

  // 2. Limits View
  function renderLimitsGrid() {
    const container = document.getElementById('view-limits');
    if (!container) return;

    if (!limitsData || limitsData.length === 0) {
      container.innerHTML = '<div style="color:var(--muted); padding:20px;">暂无提供方配额记录</div>';
      return;
    }

    // 按 provider 分组
    const grouped = {};
    for (const l of limitsData) {
      if (!grouped[l.providerId]) grouped[l.providerId] = [];
      grouped[l.providerId].push(l);
    }

    container.innerHTML = Object.entries(grouped).map(([providerId, windows]) => {
      const color = getClientColor(providerId);
      const isOk = windows[0]?.status === 'ok';

      const metersHtml = windows.map(w => {
        const rem = w.remainingPercent;
        const barColor = rem === null ? 'var(--muted)' : rem > 50 ? '#22c55e' : rem > 20 ? '#eab308' : '#ef4444';
        const barWidth = rem !== null ? Math.min(100, Math.max(0, rem)) : 0;
        const remLabel = rem !== null ? `${rem}%` : '--';

        return `
          <div style="margin-top:6px;">
            <div class="meter-label-row">
              <span>${escapeHtml(w.label || w.windowKind)}</span>
              <strong style="color:${barColor}">${remLabel}</strong>
            </div>
            <div class="progress-track" style="margin-top:3px;">
              <div class="progress-fill" style="width:${barWidth}%; background:${barColor};"></div>
            </div>
          </div>
        `;
      }).join('');

      return `
        <div class="limit-card">
          <div class="limit-head">
            <div class="limit-title">
              <span class="tool-dot" style="background:${color};"></span>
              <span>${escapeHtml(providerId.toUpperCase())}</span>
            </div>
            <span class="status-pill ${isOk ? 'status-ok' : 'status-not_configured'}">
              ${isOk ? 'OK' : 'Not Configured'}
            </span>
          </div>
          <div class="limit-meters">${metersHtml}</div>
        </div>
      `;
    }).join('');
  }

  // 3. Tools View
  function renderToolsTable() {
    const tbody = document.getElementById('tools-table-body');
    if (!tbody) return;
    if (!toolsData || toolsData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center; color:var(--muted);">暂无记录</td></tr>';
      return;
    }

    tbody.innerHTML = toolsData.map(t => `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:6px;">
            <span class="tool-dot" style="background:${getClientColor(t.id)};"></span>
            <strong>${escapeHtml(t.name)}</strong>
          </div>
        </td>
        <td><span style="color:var(--muted);">${escapeHtml(t.category)}</span></td>
        <td><strong>${formatCompactTokens(t.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(t.costUsd)}</td>
        <td>${t.sessionCount}</td>
        <td><span class="status-pill status-ok">● 正常</span></td>
      </tr>
    `).join('');
  }

  // 4. Sessions View
  function renderSessionsTable() {
    const tbody = document.getElementById('sessions-table-body');
    if (!tbody) return;
    if (!sessionsData || sessionsData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center; color:var(--muted);">暂无会话记录</td></tr>';
      return;
    }

    tbody.innerHTML = sessionsData.map(s => `
      <tr class="session-row" style="cursor:pointer;" data-session-id="${escapeHtml(s.sessionId)}">
        <td>
          <strong>${escapeHtml(s.title || s.sessionId)}</strong>
          ${s.projectPath ? `<div style="font-size:10px; color:var(--muted);">${escapeHtml(s.projectPath)}</div>` : ''}
        </td>
        <td><span style="color:var(--muted);">${escapeHtml(s.sourceId)}</span></td>
        <td><strong>${formatCompactTokens(s.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(s.costUsd)}</td>
        <td>${s.messageCount}</td>
        <td style="color:var(--muted);">${escapeHtml(s.lastUsedAt)}</td>
      </tr>
    `).join('');
  }

  async function openSessionModal(sessionId) {
    try {
      const detail = await apiCall(`/api/sessions/detail?id=${encodeURIComponent(sessionId)}`);
      document.getElementById('modal-title').textContent = `会话详情: ${sessionId}`;
      const container = document.getElementById('modal-content');

      const records = detail.records || [];
      if (records.length === 0) {
        container.innerHTML = '<p style="color:var(--muted); padding:10px;">暂无每轮明细</p>';
      } else {
        container.innerHTML = records.map(r => `
          <div class="turn-card">
            <div class="turn-header">
              <span>Turn: ${escapeHtml(r.turnId || '#')} · <code>${escapeHtml(r.model)}</code></span>
              <span>${escapeHtml(r.recordedAt)}</span>
            </div>
            <div style="display:flex; justify-content:space-between; font-size:11px;">
              <span>Input: ${formatCompactTokens(r.inputTokens)} (Cache: ${formatCompactTokens(r.cacheReadTokens)})</span>
              <span>Output: ${formatCompactTokens(r.outputTokens)}</span>
              <strong style="color:var(--accent);">Cost: ${formatMoney(r.costUsd)}</strong>
            </div>
          </div>
        `).join('');
      }

      document.getElementById('session-modal').style.display = 'flex';
    } catch (err) {
      console.error('openSessionModal failed:', err);
    }
  }

  // 5. Models View
  function renderModelsTable() {
    const tbody = document.getElementById('models-table-body');
    if (!tbody) return;
    if (!modelsData || modelsData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" style="text-align:center; color:var(--muted);">暂无模型统计</td></tr>';
      return;
    }

    tbody.innerHTML = modelsData.map(m => `
      <tr>
        <td><code>${escapeHtml(m.model)}</code></td>
        <td>${formatCompactTokens(m.inputTokens)}</td>
        <td>${formatCompactTokens(m.outputTokens)}</td>
        <td><strong>${formatCompactTokens(m.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(m.costUsd)}</td>
      </tr>
    `).join('');
  }

  // 6. Trends View
  function renderTrendsTable() {
    const tbody = document.getElementById('trends-table-body');
    if (!tbody) return;
    if (!trendsData || trendsData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" style="text-align:center; color:var(--muted);">暂无趋势数据</td></tr>';
      return;
    }

    tbody.innerHTML = trendsData.map(tr => `
      <tr>
        <td><strong>${escapeHtml(tr.date)}</strong></td>
        <td><span style="color:var(--muted);">${escapeHtml(tr.sourceId)}</span></td>
        <td><strong>${formatCompactTokens(tr.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(tr.costUsd)}</td>
        <td>${tr.sessionCount}</td>
      </tr>
    `).join('');
  }
})();
