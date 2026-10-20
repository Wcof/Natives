// Token Monitor Authentic Frontend Controller
(function () {
  'use strict';

  const {t,apply:applyLanguage}=window.TokenLocale;
  let token = '';
  let challenge, refreshGeneration=0, refreshController, detailController;
  const failures=new Set();
  let loaded=false;
  let activePeriod = 'today';
  let activeView = 'home';
  let overviewData = null;
  let toolsData = [];
  let limitsData = [];
  let sessionsData = [];
  let modelsData = [];
  let trendsData = [];

  const getClientColor=window.TokenClientColor;

  let initialized = false;
  let currentGeneration = null;

  const cancelRequests=()=>{refreshGeneration++;refreshController?.abort();detailController?.abort();};
  window.addEventListener('pagehide',cancelRequests);
  document.addEventListener('visibilitychange',()=>{if(document.hidden)cancelRequests();else if(token)void refreshAll();});

  // 接收宿主页（app.html）双阶段握手消息（契约与幂等初始化，F10）
  window.addEventListener('message', async (event) => {
    if (event.source !== window.parent) return;
    const data = event.data;
    if (!data || typeof data !== 'object') return;

    if (data.type === 'init') {
      currentGeneration = data.generation; challenge=data.challenge;
      document.documentElement.dataset.theme=data.appearance==='dark'?'volt':'archive';
      applyLanguage(data.language);
      window.parent.postMessage({
        type: 'hello',
        generation: data.generation,
        challenge: data.challenge,
      }, '*');
    } else if (data.type === 'welcome') {
      if (data.generation !== currentGeneration || data.challenge !== challenge || typeof data.token!=='string') return;
      token = data.token;
      if (!initialized) {
        initialized = true;
        setupEventListeners();
      } else { return; }
      try {
        await refreshAll();
      } catch (e) {
        console.warn('tokenusage initial refresh failed:', e);
      }
    } else if (data.type === 'language') {
      applyLanguage(data.language); renderAll();
    } else if (data.type === 'theme') {
      const theme = data.appearance === 'dark' || data.theme === 'volt' ? 'volt' : 'archive';
      document.documentElement.dataset.theme = theme;
    }
  });

  async function apiCall(route, options = {}) {
    const headers = {
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json',
      ...(options.headers || {})
    };
    const res = await fetch(route, { ...options, headers, signal:AbortSignal.any([options.signal,AbortSignal.timeout(15_000)].filter(Boolean)) });
    if (!res.ok) throw new Error(`API error: ${res.status}`);
    return res.json();
  }

  function formatNumber(n) {
    return n == null || !Number.isFinite(Number(n)) ? '--' : Number(n).toLocaleString();
  }

  function formatCompactTokens(n) {
    if (n == null || !Number.isFinite(Number(n))) return '--';
    n = Number(n);
    if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B';
    if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
    if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K';
    return n.toString();
  }

  function formatMoney(amount) {
    if (amount == null || !Number.isFinite(Number(amount))) return '--';
    const n = Number(amount);
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

  function setupEventListeners() {
    const tabs=[...document.querySelectorAll('.view-btn')];
    const syncTabs=()=>tabs.forEach(button=>{
      const selected=button.dataset.view===activeView;
      button.setAttribute('role','tab');button.setAttribute('aria-selected',String(selected));button.tabIndex=selected?0:-1;
      button.id=`tab-${button.dataset.view}`;button.setAttribute('aria-controls',`view-${button.dataset.view}`);
      document.getElementById(`view-${button.dataset.view}`).setAttribute('aria-labelledby',button.id);
      document.getElementById(`view-${button.dataset.view}`).setAttribute('role','tabpanel');
    });
    syncTabs();
    document.querySelector('.view-switcher').addEventListener('keydown',event=>{
      if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
      event.preventDefault();const index=tabs.indexOf(document.activeElement);
      const next=event.key==='Home'?0:event.key==='End'?tabs.length-1:(index+(event.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;
      tabs[next].click();tabs[next].focus();
    });
    document.getElementById('session-modal').addEventListener('close',()=>detailController?.abort());
    // Period Tabs (DAY, WEEK, MONTH, TOTAL)
    document.querySelectorAll('.period-tab').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.period-tab').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        document.querySelectorAll('.period-tab').forEach(button=>button.setAttribute('aria-pressed',String(button===btn)));
        activePeriod = btn.dataset.period || 'today';
        void refreshAll();
      });
    });

    // View Switcher (Home, Limits, Tools, Sessions, Models, Trends)
    document.querySelectorAll('.view-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.view-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        activeView = btn.dataset.view || 'home';
        switchView(activeView);syncTabs();
      });
    });

    // Refresh Button
    document.getElementById('btn-refresh')?.addEventListener('click', async () => {
      const btn = document.getElementById('btn-refresh');
      btn.disabled = true;
      try {
        await refreshAll();
      } catch (err) {
        console.error('Refresh failed:', err);
      } finally {
        btn.disabled = false;
      }
    });

    // Session Modal Close
    document.getElementById('btn-modal-close')?.addEventListener('click', () => {
      document.getElementById('session-modal').close();
      detailController?.abort();
    });
    document.getElementById('session-modal')?.addEventListener('click', (e) => {
      if (e.target.id === 'session-modal') {
        document.getElementById('session-modal').close();
      detailController?.abort();
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

  function showStatusBanner(message, type = 'info', retryable = false) {
    let banner = document.getElementById('status-banner');
    if (!banner) {
      banner = document.createElement('div');
      banner.id = 'status-banner';
      banner.className = 'status-banner';
      const main = document.querySelector('main');
      if (main) main.prepend(banner);
    }
    banner.className = `status-banner status-${type}`;
    banner.style.display = 'block';
    banner.setAttribute('role',type==='error'?'alert':'status');
    banner.innerHTML=`<span>${escapeHtml(message)}</span>${retryable ? `<button id="btn-status-retry" type="button">${t('retry')}</button>` : ''}`;

    if (retryable) {
      document.getElementById('btn-status-retry')?.addEventListener('click', () => refreshAll());
    }
  }

  function hideStatusBanner() {
    const banner = document.getElementById('status-banner');
    if (banner) banner.style.display = 'none';
  }

  const periodQuery=()=>`range=${{today:'today',week:'7d',month:'30d',allTime:'all'}[activePeriod]}`;
  function renderAll(){
    if(!loaded)return;
    updateTotalPanel();renderHomeBreakdown();renderLimitsGrid();renderToolsTable();renderSessionsTable();renderModelsTable();renderTrendsTable();
  }
  async function refreshAll() {
    refreshController?.abort(); refreshController=new AbortController();
    const generation=++refreshGeneration;
    showStatusBanner(t('loading'));
    const routes=['overview','tools','limits','sessions','models','trends'];
    const results=[];
    for(let offset=0;offset<routes.length;offset+=2){
      if(generation!==refreshGeneration || refreshController.signal.aborted)return;
      results.push(...await Promise.allSettled(routes.slice(offset,offset+2).map(route=>apiCall(`/api/${route}?${periodQuery()}`,{signal:refreshController.signal}))));
    }
    if(generation!==refreshGeneration || refreshController.signal.aborted)return;
    const missing=results.map((result,index)=>result.status==='rejected'?index:-1).filter(index=>index>=0);
    if(missing.length){
      showStatusBanner(t('retrying'));
      const signal=refreshController.signal;
      await new Promise(resolve=>{
        const finish=()=>{clearTimeout(timer);signal.removeEventListener('abort',finish);resolve();};
        const timer=setTimeout(finish,1100);signal.addEventListener('abort',finish,{once:true});
      });
      if(generation!==refreshGeneration || signal.aborted)return;
      const retried=await Promise.allSettled(missing.map(index=>apiCall(`/api/${routes[index]}?${periodQuery()}`,{signal})));
      retried.forEach((result,index)=>{results[missing[index]]=result;});
      if(generation!==refreshGeneration || signal.aborted)return;
    }
    failures.clear();
    results.forEach((result,index)=>{if(result.status==='rejected')failures.add(routes[index]);});
    if(results[0].status==='fulfilled')overviewData=results[0].value;
    if(results[1].status==='fulfilled')toolsData=results[1].value.tools || [];
    if(results[2].status==='fulfilled')limitsData=results[2].value.limits || [];
    if(results[3].status==='fulfilled')sessionsData=results[3].value.sessions || [];
    if(results[4].status==='fulfilled')modelsData=results[4].value.models || [];
    if(results[5].status==='fulfilled')trendsData=results[5].value.trends || [];
    loaded=true;renderAll();
    document.getElementById('live-dot').hidden=failures.size>0;
    document.getElementById('status-text').textContent=failures.size?t('partial'):`${t('updated')} ${new Date().toLocaleTimeString()}`;
    if(failures.size)showStatusBanner(t(failures.size===routes.length?'unavailable':'partial'),'error',true);
    else hideStatusBanner();
  }

  function updateTotalPanel() {
    if (!overviewData) return;
    const periods = overviewData.periods || {};
    let currentPeriod = periods.today || {};
    if (activePeriod === 'week') currentPeriod = periods.thisWeek || {};
    else if (activePeriod === 'month') currentPeriod = periods.thisMonth || {};
    else if (activePeriod === 'allTime') currentPeriod = periods.allTime || {};

    const totalEl = document.getElementById('totalTokens');
    const costEl = document.getElementById('cost');

    if (totalEl) totalEl.textContent = formatNumber(currentPeriod.totalTokens);
    if (costEl) costEl.textContent = formatMoney(currentPeriod.costUsd);
  }

  // 1. Home View Breakdown (Signature Token Monitor Style)
  function renderHomeBreakdown() {
    const container = document.getElementById('view-home');
    if (!container) return;

    const activeTools=toolsData.filter(tool=>tool.totalTokens>0);
    if (!activeTools.length) {
      container.innerHTML = `<div style="text-align:center; padding:30px; color:var(--muted);">${t(failures.has('tools')?'unavailable':'noTools')}</div>`;
      return;
    }

    const maxTokens = Math.max(...activeTools.map(t => t.totalTokens || 0), 1);

    container.innerHTML = activeTools.map(t => {
      const color = getClientColor(t.id);
      const percent = Math.min(100, Math.max(0, Math.round((t.totalTokens / maxTokens) * 100)));

      return `
        <div class="tool-row">
          <div class="tool-row-head">
            <div class="tool-brand">
              <span class="tool-dot" style="background:${color};"></span>
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
      container.innerHTML = `<div style="color:var(--muted); padding:20px;">${t(failures.has('limits')?'unavailable':'noLimits')}</div>`;
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
        const rem = Number.isFinite(w.remainingPercent) ? w.remainingPercent : null;
        const barColor = rem === null ? 'var(--muted)' : rem > 50 ? 'var(--natives-ok)' : rem > 20 ? 'var(--natives-warning)' : 'var(--natives-danger)';
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
              ${isOk ? t('normal') : t('notConfigured')}
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
      tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--muted);">${t(failures.has('tools')?'unavailable':'noRecords')}</td></tr>`;
      return;
    }

    tbody.innerHTML = toolsData.map(tool => `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:6px;">
            <span class="tool-dot" style="background:${getClientColor(tool.id)};"></span>
            <strong>${escapeHtml(tool.name)}</strong>
          </div>
        </td>
        <td><span style="color:var(--muted);">${escapeHtml(tool.category || '--')}</span></td>
        <td><strong>${formatCompactTokens(tool.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(tool.costUsd)}</td>
        <td>${tool.requestCount}</td>
        <td><span class="status-pill status-ok">${t(tool.enabled?'normal':'disabled')}</span></td>
      </tr>
    `).join('');
  }

  // 4. Sessions View
  function renderSessionsTable() {
    const tbody = document.getElementById('sessions-table-body');
    if (!tbody) return;
    if (!sessionsData || sessionsData.length === 0) {
      tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--muted);">${t(failures.has('sessions')?'unavailable':'noSessions')}</td></tr>`;
      return;
    }

    tbody.innerHTML = sessionsData.map(s => `
      <tr class="session-row" style="cursor:pointer;" data-session-id="${escapeHtml(s.sessionId)}">
        <td>
          <button type="button" class="session-detail-button">${escapeHtml(s.title || s.sessionId)}</button>
          ${s.projectPath ? `<div style="font-size:12px; color:var(--muted);">${escapeHtml(s.projectPath)}</div>` : ''}
        </td>
        <td><span style="color:var(--muted);">${escapeHtml(s.sourceId)}</span></td>
        <td><strong>${formatCompactTokens(s.totalTokens)}</strong></td>
        <td style="color:var(--accent);">${formatMoney(s.costUsd)}</td>
        <td>${s.messageCount}</td>
        <td style="color:var(--muted);">${escapeHtml(s.lastUsedAt)}</td>
      </tr>
    `).join('');
  }

  async function openSessionModal(sessionId, offset = 0) {
    detailController?.abort();
    const owned = new AbortController();
    detailController = owned;
    const dialog = document.getElementById('session-modal');
    const container = document.getElementById('modal-content');
    const previousFocus = document.activeElement?.dataset.detailPage;
    document.getElementById('modal-title').textContent = `${t('sessionDetails')}: ${sessionId}`;
    container.textContent = t('loading');
    dialog.setAttribute('aria-busy', 'true');
    if (!dialog.open) dialog.showModal();
    try {
      const detail = await apiCall(`/api/sessions/detail?id=${encodeURIComponent(sessionId)}&offset=${offset}`, { signal: owned.signal });
      if (owned.signal.aborted) return;
      renderSessionRecords(container, detail.records || []);
      const nav = document.createElement('nav');
      for (const [name, value] of [['previousPage', offset - 50], ['nextPage', detail.hasMore ? detail.nextOffset : null]]) {
        if (value == null || value < 0) continue;
        const button = document.createElement('button');
        button.type = 'button';
        button.dataset.detailPage = name;
        button.textContent = t(name);
        button.onclick = () => openSessionModal(sessionId, value);
        nav.append(button);
      }
      container.append(nav);
      if (previousFocus) (nav.querySelector(`[data-detail-page="${previousFocus}"]`) || nav.querySelector('button') || document.getElementById('btn-modal-close')).focus();
    } catch (error) {
      if (owned.signal.aborted) return;
      container.textContent = t('detailFailed');
      const retry = document.createElement('button');
      retry.type = 'button';
      retry.textContent = t('retry');
      retry.onclick = () => openSessionModal(sessionId, offset);
      container.append(retry);
    } finally {
      if (detailController === owned) dialog.setAttribute('aria-busy', 'false');
    }
  }

  function renderSessionRecords(container, records) {
    container.innerHTML = records.length ? records.map(record => `
      <div class="turn-card">
        <div class="turn-header">
          <span>${t('turn')}: ${escapeHtml(record.turnId || '#')} · <code>${escapeHtml(record.model)}</code></span>
          <span>${escapeHtml(record.recordedAt)}</span>
        </div>
        <div style="display:flex; justify-content:space-between; font-size:12px;">
          <span>${t('input')}: ${formatCompactTokens(record.inputTokens)} (${t('cache')}: ${formatCompactTokens(record.cacheReadTokens)})</span>
          <span>${t('output')}: ${formatCompactTokens(record.outputTokens)}</span>
          <strong style="color:var(--accent);">${t('cost')}: ${formatMoney(record.costUsd)}</strong>
        </div>
      </div>`).join('') : `<p>${t('noTurns')}</p>`;
  }

  // 5. Models View
  function renderModelsTable() {
    const tbody = document.getElementById('models-table-body');
    if (!tbody) return;
    if (!modelsData || modelsData.length === 0) {
      tbody.innerHTML = `<tr><td colspan="5" style="text-align:center; color:var(--muted);">${t(failures.has('models')?'unavailable':'noModels')}</td></tr>`;
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
      tbody.innerHTML = `<tr><td colspan="5" style="text-align:center; color:var(--muted);">${t(failures.has('trends')?'unavailable':'noTrends')}</td></tr>`;
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
