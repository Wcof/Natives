

import { escapeHtml } from '../sanitizer.js';

export const topSitesWidget = {
  key: 'widget/topSites',
  name: 'Top Sites',
  defaultData: {
    limit: 6,
  },
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    let disposed = false;
    const limit = Math.max(1, Math.min(18, Number(data.limit) || 6));

    container.className = 'Widget TopSites';
    container.replaceChildren();

    const root = document.createElement('div');
    root.className = 'Links';
    container.append(root);

    function renderSites(permissionChecked = false) {
      if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;
      if (!permissionChecked && globalThis.chrome?.permissions?.contains) {
        chrome.permissions.contains({ permissions: ['topSites'] }, (granted) => {
          if (disposed) return;
          if (granted) renderSites(true);
          else renderPermissionPrompt();
        });
        return;
      }
      if (globalThis.chrome?.topSites) {
        chrome.topSites.get((sites) => {
          if (disposed) return;
          root.replaceChildren();

          if (!sites || !sites.length) {
            root.innerHTML = `<div class="top-sites-empty">${t('noTopSites', '暂无常用网址')}</div>`;
            return;
          }

          const grid = document.createElement('div');
          grid.className = 'top-sites-list';

          sites.slice(0, limit).forEach((site) => {
            if (!site.url) return;
            const domain = extractDomain(site.url);

            const a = document.createElement('a');
            a.className = 'Link top-site-item';
            a.href = site.url;
            a.target = '_blank';
            a.rel = 'noopener noreferrer';
            a.title = site.title || domain || site.url;

            const icon = document.createElement('img');
            icon.className = 'custom-icon top-site-favicon';
            icon.src = `https://www.google.com/s2/favicons?domain=${encodeURIComponent(domain)}&sz=64`;
            icon.alt = '';

            const titleSpan = document.createElement('span');
            titleSpan.className = 'top-site-title';
            titleSpan.textContent = site.title || domain || 'Site';

            a.append(icon, titleSpan);
            grid.append(a);
          });

          root.append(grid);
        });
      } else {
        renderPermissionPrompt();
      }
    }

    function renderPermissionPrompt() {
      root.replaceChildren();
      const permBtn = document.createElement('button');
      permBtn.type = 'button';
      permBtn.className = 'top-sites-perm-btn request-permission';
      permBtn.textContent = t('clickToAuthorizeTopSites', '点击授权常访问网址权限');
      permBtn.onclick = () => {
        chrome.permissions?.request?.({ permissions: ['topSites'] }, (granted) => {
          if (granted) renderSites(true);
        });
      };
      root.append(permBtn);
    }

    renderSites();

    return () => {
      disposed = true;
      container.replaceChildren();
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <label class="inspector-field">
        <span>${t('displayLimit', '显示数量 (1-18)')}</span>
        <input type="number" id="ts-limit" min="1" max="18" value="${data.limit || 6}" />
      </label>
    `;
    wrap.querySelector('#ts-limit').onchange = (e) => onChange({ ...data, limit: Number(e.target.value) || 6 });
    container.append(wrap);
  },
  styles: `
    .TopSites { text-align:left; }
    .TopSites .top-sites-list { display:contents; }
    .TopSites .top-sites-perm-btn {
      background-color:var(--bg-secondary);
      color:var(--text-heading);
      border:2px solid var(--text-heading);
      margin:.6em 0;
      border-radius:8px;
      cursor: pointer;
      font-weight:500;
      padding:10px 12px;
      text-align:center;
      transition:all 200ms;
    }
    .TopSites .top-sites-empty {
      font-size: 12px;
      opacity: 0.7;
    }
  `,
};

function extractDomain(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}
