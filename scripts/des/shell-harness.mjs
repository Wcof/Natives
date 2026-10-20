// DES 外壳装配 harness（T02 起）：按 ui.rs 的编译期顺序在真实 Chrome 中
// 装配生产 partials 与 src JS，fetch 全部返回合成空数据。仅供壳层交互
// 验证；业务数据路径由各专项 fixture 测试覆盖（N04～N08 及 A06+）。
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const SRC = resolve(import.meta.dirname, '../../modules/fund/ui/src');

// 与 ui.rs BUNDLE_JS 保持一致（新模块文件需同步 ui.rs）。
const BUNDLE_JS = [
  'core.js', 'research-series.js', 'drawing-session.js', 'market-session.js', 'dashboard.js', 'watchlist-table.js', 'market-table.js',
  'chart.js', 'chart-drawings.js', 'ledger.js', 'positions-menu.js', 'positions-income.js', 'fund-classification.js',
  'positions-edit.js', 'positions-view.js', 'otc-ops.js', 'assets.js', 'focus.js', 'trends-chart.js',
  'trends-kline.js', 'trends-minute.js', 'trends-interact.js', 'trends-indicator.js',
  'trends-chan.js', 'trends-flow.js', 'trends-drawings.js', 'trends-panel.js', 'trends-watch.js',
  'trends-scan.js', 'trends.js', 'home-i18n.js', 'home-overview.js', 'home-opportunities.js',
  'home.js', 'home-range.js', 'home-manage.js', 'home-evidence.js', 'home-report-feedback.js', 'home-notifications.js',
  'settings-view.js', 'settings-diagnostics.js', 'settings-network.js', 'settings-templates.js',
  'fund-detail-chart.js', 'fund-detail.js', 'holding-summary.js', 'stock-drawings-shapes.js', 'stock-drawings.js', 'stock-detail-chart.js', 'stock-detail.js', 'app.js',
];
// ui.rs INDEX_HTML 顺序（不含 app.js script 标签）。
const INDEX_PARTIALS = [
  'partials/head.html', 'partials/svg-symbols.html', 'partials/header.html',
  'partials/view-home.html', 'partials/view-watchlist.html', 'partials/view-market.html',
  'partials/view-stock-detail.html', 'partials/view-fund-detail.html', 'partials/view-trends.html', 'partials/view-positions.html',
  'partials/view-assets.html', 'partials/view-settings.html', 'partials/view-tx.html',
  'partials/view-import.html', 'partials/view-nav.html', 'partials/side-panel.html',
  'partials/overlays.html',
];

export function shellCss() {
  return readFileSync(resolve(SRC, '../dist/styles/theme.css'), 'utf8') + '\n'
    + ['layout', 'panel', 'trends', 'trends-cards', 'stock-detail', 'fund-detail', 'home', 'positions', 'investment']
      .map((n) => readFileSync(resolve(SRC, `styles/${n}.css`), 'utf8')).join('\n');
}

export function shellHtml() {
  // head.html 已含 doctype/head 开标签但没有闭合标签；ui.rs 用胶水行
  // 闭合 head 并开 body（见 INDEX_HTML concat!）。这里保持同一语义。
  const head = readFileSync(resolve(SRC, 'partials/head.html'), 'utf8');
  return head
    + `<style>${shellCss()}</style>\n</head>\n<body>\n`
    + INDEX_PARTIALS.slice(1).map((p) => readFileSync(resolve(SRC, p), 'utf8')).join('\n')
    + '\n<script src="app.js"></script>\n</body>\n</html>';
}

export function shellScripts() {
  return BUNDLE_JS;
}

export async function installShellStubs(page) {
  await page.addInitScript(() => {
    window.requests = [];
    const ok = (body) => ({ ok: true, status: 200, json: async () => body ?? ({}), text: async () => JSON.stringify(body ?? {}) });
    window.fetch = async (url, options = {}) => {
      const path = new URL(url, 'http://localhost').pathname + new URL(url, 'http://localhost').search;
      window.requests.push({ method: options.method || 'GET', path });
      return ok({});
    };
    window.addEventListener('message', (e) => {
      if (e.data?.type === 'hello') {
        window.parent !== window && window.parent.postMessage({ type: 'welcome', token: 'synthetic-token', generation: e.data.generation }, '*');
      }
    });
  });
}
