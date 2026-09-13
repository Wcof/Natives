// R8 视觉验收（方案 §5.5/§9.0.1）：真实浏览器 + 真实 Shadow DOM 证据。
//
// 加载真实 space-dashboard.js（非 DOM mock），在系统 Chrome 中渲染七个
// 独立 AI 组件 key，验证：
//   1. 七个 key 各自独立挂载、内容互不串用（成本/Token/会话/调用四张
//      usage 卡同屏，limits/attention/savings 三张视图卡同屏）；
//   2. 每张卡消费空间解析后的局部角色（--space-widget-text 由
//      applyWidgetDisplayStyles 注入卡片根，而非全局 --text）；
//   3. 两张卡不同 colour 显式外观互不污染（§9.0.1 视觉场景）；
//   4. 计算样式对比度：正文在深/浅两种空间背景下 ≥4.5:1（§5.5）；
//   5. 空数据状态：empty 显示文案而非 0 冒充。
//
// 证据产物：本测试退出码 + 输出的 JSON 计算样式记录（tests/artifacts/）。

import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');

// file:// 下 ESM 模块被 CORS 拒绝（origin 'null'），必须经 HTTP 加载。
const MIME = {
  '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json',
  '.html': 'text/html', '.css': 'text/css',
};
const server = createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const filePath = join(extRoot, urlPath);
  import('node:fs').then(({ readFileSync }) => {
    try {
      const body = readFileSync(filePath);
      res.writeHead(200, { 'content-type': MIME[extname(filePath)] || 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end('not found');
    }
  });
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

const SEVEN_KEYS = [
  'widget/aiCost',
  'widget/aiTokens',
  'widget/aiSessions',
  'widget/aiRequests',
  'widget/aiLimits',
  'widget/aiAttention',
  'widget/aiSavings',
];

// 模型宿主未运行时 renderer 走 error/empty 路径——这正是我们要验证的状态之一。
// nativeCall 返回显式错误，卡内不得冒充 0。
const stubNativeCall = async (method) => {
  throw new Error(`model-host unavailable in test harness (${method})`);
};

const snapshotFor = (displayOverrides = {}, explicitColours = false, bgHex = '#101210') => ({
  revision: 1,
  // colourBackground 插件读取 display.colour——必须用正确字段传入，
  // 否则背景层回退默认 #101010，浅色场景对比度基线失真。
  backgroundJson: { key: 'background/colour', display: { colour: bgHex, brightness: 1 } },
  widgets: SEVEN_KEYS.map((key, i) => ({
    id: `w-${i}`,
    key,
    enabled: true,
    configJson: {},
    // 默认场景不设 colour：卡片文字色由空间背景派生（§5.2 解析链），
    // 对比度必须达标。显式覆盖场景单独注入颜色，只验证"保留"而非对比度。
    displayJson: {
      position: 'free',
      xPercent: 12 + i * 11,
      yPercent: 40,
      ...(explicitColours ? (i % 2 === 0 ? { colour: '#ffffff' } : { colour: '#ffd166' }) : {}),
      ...displayOverrides,
    },
  })),
});


async function runScenario(browser, name, backgroundHex, displayOverrides = {}, explicitColours = false) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error(`[${name} pageerror]`, String(e).slice(0, 300)));
  page.on('requestfailed', (r) => console.error(`[${name} reqfail]`, r.url().slice(-80), r.failure()?.errorText));
  // 经本地 HTTP 加载真实扩展模块（file:// 下 ESM 被 CORS 拒绝）。
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    // 真实 space.html 中宿主是 flex item（space.css .dashboard-host），被 blockified；
    // shadow 的 `:host{all:initial}` 会把裸 div 压成 inline——必须复刻 flex 父级。
    `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
     <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
     <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
  );
  writeFileSync(
    join(extRoot, 'space-verify-boot.mjs'),
    `import { createSpaceDashboard } from './space-dashboard.js';
     import { widgetPlugins } from './plugins/widgets/index.js';
     import { backgroundPlugins } from './plugins/backgrounds/index.js';
     window.__createDashboard = createSpaceDashboard;
     window.__widgetPlugins = widgetPlugins;
     window.__backgroundPlugins = backgroundPlugins;`
  );
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

  const records = await page.evaluate(
    async ({ snapshot, bg }) => {
      const host = document.getElementById('dashboard-host');
      host.style.width = '1200px';
      host.style.height = '700px';
      host.style.background = bg;
      const dashboard = window.__createDashboard({
        // space.js 传入的 $ 接收裸 id（如 'dashboard-host'），需按 id 解析。
        $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
        t: (key, fallback) => fallback || key,
        selectedLanguage: 'zh_CN',
        backgroundPlugins: window.__backgroundPlugins,
        widgetPlugins: window.__widgetPlugins,
        nativeCall: async (method) => {
          throw new Error('model-host unavailable (' + method + ')');
        },
        broadcastRevision: () => {},
        queueWorkspaceMutation: () => {},
        toast: () => {},
      });
      dashboard.render(snapshot, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 400));

      const shadow = host.shadowRoot;
      // canvas fillStyle 可把任意 CSS 颜色（oklch/color-mix/var 解析结果等）
      // 规范化为 rgb()，供 Node 端做亮度/对比度计算。
      const colorCtx = document.createElement('canvas').getContext('2d');
      const resolveColor = (str) => {
        if (!str) return null;
        colorCtx.fillStyle = '#000000';
        colorCtx.fillStyle = str;
        const v = colorCtx.fillStyle;
        const m = /rgba?\(([^)]+)\)/.exec(v);
        if (m) {
          const parts = m[1].split(',').map((x) => parseFloat(x));
          return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
        }
        const h = /^#([0-9a-f]{6})$/i.exec(v);
        if (h) {
          const n = parseInt(h[1], 16);
          return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
        }
        return null;
      };
      // 有效文字色：容器自身可能继承透明色，找第一个有非空直接文本的后代。
      const effectiveColor = (el) => {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_ELEMENT);
        let node = el;
        while (node) {
          const hasOwnText = Array.from(node.childNodes).some(
            (c) => c.nodeType === Node.TEXT_NODE && c.textContent.trim().length > 0
          );
          if (hasOwnText) {
            const c = resolveColor(getComputedStyle(node).color);
            if (c && c.a > 0) return { rgb: c, from: node.tagName + '.' + node.className };
          }
          node = walker.nextNode();
        }
        return { rgb: resolveColor(getComputedStyle(el).color), from: 'container' };
      };
      const out = [];
      // free 布局下外层 wrapper（.Slot.free）与内层 container 都带 data-widget-id；
      // 只统计内层渲染容器，避免 2 倍计数。
      for (const el of shadow.querySelectorAll('[data-widget-id]:not(.Slot)')) {
        const cs = getComputedStyle(el);
        const eff = effectiveColor(el);
        out.push({
          widgetId: el.dataset.widgetId,
          textSample: el.textContent.replace(/\s+/g, ' ').trim().slice(0, 60),
          injectedTextVar: el.style.getPropertyValue('--space-widget-text') || null,
          computedColor: cs.color,
          effectiveColor: eff.rgb,
          effectiveFrom: eff.from,
          fontSize: cs.fontSize,
        });
      }
      return out;
    },
    { snapshot: snapshotFor(displayOverrides, explicitColours, backgroundHex), bg: backgroundHex }
  );

  // --- 断言 ---
  assert.equal(records.length, 7, `${name}: must mount exactly 7 widgets, got ${records.length}`);

  // 每张卡都被空间层注入了局部角色变量（§5.2 桥接证据）。
  for (const rec of records) {
    assert.ok(
      rec.injectedTextVar && rec.injectedTextVar.length > 0,
      `${name}: ${rec.widgetId} missing --space-widget-text injection`
    );
  }

  // 相邻两卡不同 colour 不串色（§9.0.1）：仅在显式覆盖场景有意义。
  const colorSet = new Set(records.map((r) => r.injectedTextVar));
  if (explicitColours) {
    assert.ok(colorSet.size >= 2, `${name}: adjacent different-colour cards must not share injected color`);
  } else {
    assert.equal(colorSet.size, 1, `${name}: default scenario must derive ONE space-resolved text color`);
  }

  // 错误/空状态有文字而非空白（host 不可达时不得静默或冒充 0）。
  for (const rec of records) {
    assert.ok(rec.textSample.length > 0, `${name}: ${rec.widgetId} rendered empty content`);
  }

  // 对比度（§5.5）：有效文字色 vs 空间背景的相对亮度比 ≥ 4.5。
  // alpha 与页面背景合成后再计算（评估的是合成后的实际背景）。
  const luminance = ({ r, g, b }) => {
    const f = (v) => {
      const s = v / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const contrast = (fg, bgL) => {
    if (!fg) return null;
    const fgL = luminance(fg);
    const [hi, lo] = fgL > bgL ? [fgL, bgL] : [bgL, fgL];
    return (hi + 0.05) / (lo + 0.05);
  };
  const bgRgb = (() => {
    const h = /^#([0-9a-f]{6})$/i.exec(backgroundHex);
    const n = parseInt(h[1], 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
  })();
  const bgL = luminance(bgRgb);
  const ratios = records.map((rec) => ({
    widgetId: rec.widgetId,
    from: rec.effectiveFrom,
    ratio: contrast(rec.effectiveColor, bgL),
  }));
  for (const { widgetId, ratio, from } of ratios) {
    assert.ok(
      ratio != null && ratio >= 4.5,
      `${name}: ${widgetId} (text from ${from}) contrast ${ratio?.toFixed?.(2)} < 4.5:1 against ${backgroundHex}`
    );
  }

  await context.close();
  return { scenario: name, background: backgroundHex, records, contrastRatios: ratios };
}

// §5.5 动态切换 + §9.0.1"改外观不发数据请求"：同一 dashboard 实例上
// 往返切换空间背景，断言（a）nativeCall 次数不变（外观变化不查数）、
// （b）局部角色随新背景即时更新、（c）7 卡仍挂载且状态一致。
async function runAppearanceToggleScenario(browser) {
  const name = 'appearance-toggle';
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error(`[${name} pageerror]`, String(e).slice(0, 300)));
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
     <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
     <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
  );
  writeFileSync(
    join(extRoot, 'space-verify-boot.mjs'),
    `import { createSpaceDashboard } from './space-dashboard.js';
     import { widgetPlugins } from './plugins/widgets/index.js';
     import { backgroundPlugins } from './plugins/backgrounds/index.js';
     window.__createDashboard = createSpaceDashboard;
     window.__widgetPlugins = widgetPlugins;
     window.__backgroundPlugins = backgroundPlugins;`
  );
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

  const result = await page.evaluate(
    async ({ snapshotDark, snapshotLight }) => {
      const host = document.getElementById('dashboard-host');
      host.style.width = '1200px';
      host.style.height = '700px';
      let nativeCalls = 0;
      const dashboard = window.__createDashboard({
        $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
        t: (key, fallback) => fallback || key,
        selectedLanguage: 'zh_CN',
        backgroundPlugins: window.__backgroundPlugins,
        widgetPlugins: window.__widgetPlugins,
        nativeCall: async (method) => {
          nativeCalls++;
          throw new Error('model-host unavailable (' + method + ')');
        },
        broadcastRevision: () => {},
        queueWorkspaceMutation: () => {},
        toast: () => {},
      });
      const sample = () => {
        const shadow = host.shadowRoot;
        const out = [];
        for (const el of shadow.querySelectorAll('[data-widget-id]:not(.Slot)')) {
          out.push({
            widgetId: el.dataset.widgetId,
            injectedTextVar: el.style.getPropertyValue('--space-widget-text') || null,
          });
        }
        return out;
      };
      host.style.background = '#101210';
      dashboard.render(snapshotDark, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 400));
      const callsBefore = nativeCalls;
      const darkSample = sample();

      // 往返切换：深 → 浅 → 深（§5.5 动态切换样本）。
      host.style.background = '#f4f1e8';
      dashboard.render(snapshotLight, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 400));
      const lightSample = sample();
      host.style.background = '#101210';
      dashboard.render(snapshotDark, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 400));
      const backSample = sample();

      return { callsBefore, callsAfter: nativeCalls, darkSample, lightSample, backSample };
    },
    {
      snapshotDark: snapshotFor({}, false, '#101210'),
      snapshotLight: snapshotFor({}, false, '#f4f1e8'),
    }
  );

  assert.equal(result.darkSample.length, 7, `${name}: 7 widgets before toggle`);
  assert.equal(result.lightSample.length, 7, `${name}: 7 widgets after toggle`);
  assert.equal(result.backSample.length, 7, `${name}: 7 widgets after round-trip`);
  // 核心断言：外观往返切换全程零数据请求（§5.3：改样式不触发采集/用量查询）。
  assert.equal(
    result.callsAfter, result.callsBefore,
    `${name}: appearance toggle must not trigger data requests (${result.callsBefore} -> ${result.callsAfter})`
  );
  // 局部角色随背景派生更新：深/浅两态的注入文字色必须不同，往返后恢复。
  const darkVars = new Set(result.darkSample.map((r) => r.injectedTextVar));
  const lightVars = new Set(result.lightSample.map((r) => r.injectedTextVar));
  const backVars = new Set(result.backSample.map((r) => r.injectedTextVar));
  assert.equal(darkVars.size, 1, `${name}: dark state derives one text color`);
  assert.equal(lightVars.size, 1, `${name}: light state derives one text color`);
  assert.notEqual(
    [...darkVars][0], [...lightVars][0],
    `${name}: derived text color must adapt to background`
  );
  assert.equal(
    [...backVars][0], [...darkVars][0],
    `${name}: round-trip restores dark-derived color (no stale snapshot)`
  );

  await context.close();
  return { scenario: name, callsBefore: result.callsBefore, callsAfter: result.callsAfter };
}

// §5.5 字体设置维度：fontSize/fontWeight 显式覆盖必须传导到主指标
// （--space-widget-metric-size / --space-widget-weight），且未设置时用
// renderer 默认（22px/700），不得被全局 Files 字号吞掉。
async function runFontSettingsScenario(browser) {
  const name = 'font-settings';
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error(`[${name} pageerror]`, String(e).slice(0, 300)));
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
     <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
     <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
  );
  writeFileSync(
    join(extRoot, 'space-verify-boot.mjs'),
    `import { createSpaceDashboard } from './space-dashboard.js';
     import { widgetPlugins } from './plugins/widgets/index.js';
     import { backgroundPlugins } from './plugins/backgrounds/index.js';
     window.__createDashboard = createSpaceDashboard;
     window.__widgetPlugins = widgetPlugins;
     window.__backgroundPlugins = backgroundPlugins;`
  );
  // 真实 Host 导出的契约 fixture（§9.4）：让四张 usage 卡进入 ready 数字视图，
  // 主指标 .ai-perf-number-value 才存在，字体传导才可测。
  // 根因：widget 数据层走 client.js → chrome.runtime.connectNative（真实
  // Native Messaging 通道），不经过 dashboard 的 nativeCall——必须在页面
  // 加载前把 chrome.runtime mock 注入 boot HTML（回包协议 {id, ok, result}，
  // 与 native-client.js 的 pending 解析一致；无 handshake，无需握手回包）。
  const { readFileSync } = await import('node:fs');
  const hostFixture = JSON.parse(readFileSync(join(extRoot, 'ai-performance', 'host-fixture.json'), 'utf8'));
  writeFileSync(
    join(extRoot, 'space-verify-native-fixture.json'),
    JSON.stringify({
      overview: hostFixture.overview,
      events: hostFixture.events,
      sessions: { status: 'ready', totalSessions: 2, activeDays: 1, unattributed: 0, byDay: [], bySource: [] },
    })
  );
  const bootHtml = readFileSync(join(extRoot, 'space-verify-boot.html'), 'utf8');
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    bootHtml.replace(
      '</body>',
      `<script>
        window.__nativeFixture = null;
        fetch('/space-verify-native-fixture.json').then((r) => r.json()).then((d) => { window.__nativeFixture = d; });
        window.chrome = {
          runtime: {
            connectNative: () => {
              const listeners = new Set();
              return {
                onMessage: { addListener: (fn) => listeners.add(fn) },
                onDisconnect: { addListener: () => {} },
                postMessage: (msg) => {
                  const respond = () => {
                    if (!window.__nativeFixture) return;
                    const f = window.__nativeFixture;
                    const result = msg.method === 'model_usage_overview' ? f.overview
                      : msg.method === 'model_usage_events' ? f.events
                      : msg.method === 'model_usage_sessions' ? f.sessions
                      : msg.method === 'model_usage_analysis' ? { byModel: [], bySource: [], byProvider: [], byHour: [] }
                      : { status: 'ready' };
                    for (const fn of [...listeners]) fn({ id: msg.id, ok: true, result });
                  };
                  if (window.__nativeFixture) respond();
                  else setTimeout(respond, 50);
                },
                disconnect: () => {},
              };
            },
          },
        };
      </script></body>`
    )
  );
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

  const result = await page.evaluate(
    async ({ snapshotDefault, snapshotCustom }) => {
      const host = document.getElementById('dashboard-host');
      host.style.width = '1200px';
      host.style.height = '700px';
      host.style.background = '#101210';
      // 全局 Files 字号干扰源：宿主设置大字号，卡片必须用自己的局部档位。
      host.style.fontSize = '32px';
      const dashboard = window.__createDashboard({
        $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
        t: (key, fallback) => fallback || key,
        selectedLanguage: 'zh_CN',
        backgroundPlugins: window.__backgroundPlugins,
        widgetPlugins: window.__widgetPlugins,
        nativeCall: async (method) => {
          throw new Error('unexpected dashboard nativeCall ' + method);
        },
        broadcastRevision: () => {},
        queueWorkspaceMutation: () => {},
        toast: () => {},
      });
      const metricSample = () => {
        const shadow = host.shadowRoot;
        const out = [];
        for (const el of shadow.querySelectorAll('[data-widget-id]:not(.Slot)')) {
          const num = el.querySelector('.ai-perf-number-value');
          if (!num) continue;
          const cs = getComputedStyle(num);
          out.push({
            widgetId: el.dataset.widgetId,
            metricFontSize: cs.fontSize,
            metricFontWeight: cs.fontWeight,
            inheritedContainerFontSize: getComputedStyle(el).fontSize,
          });
        }
        return out;
      };
      dashboard.render(snapshotDefault, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 800));
      const defaultSample = metricSample();
      dashboard.render(snapshotCustom, 'ws-test', () => {});
      await new Promise((r) => setTimeout(r, 800));
      const customSample = metricSample();
      // 诊断：卡片实际渲染状态（error/empty/loading 文本）与 mock 就绪情况。
      const debug = [];
      for (const el of host.shadowRoot.querySelectorAll('[data-widget-id]:not(.Slot)')) {
        debug.push({ id: el.dataset.widgetId, cls: el.className, text: el.textContent.replace(/\s+/g, ' ').slice(0, 80) });
      }
      return { defaultSample, customSample, fixtureReady: Boolean(window.__nativeFixture), debug };
    },
    {
      snapshotDefault: snapshotFor({}, false, '#101210'),
      // 字体设置场景：主指标 36px/400（§5.5：默认/加大主数字、400/600/700 样本）。
      snapshotCustom: snapshotFor({ fontSize: 36, fontWeight: 400 }, false, '#101210'),
    }
  );

  assert.ok(result.defaultSample.length >= 3, `${name}: usage cards with metric value must exist, got ${result.defaultSample.length}`);
  // 默认态：主指标用 renderer 局部默认 22px/700，不被宿主 32px 传染。
  for (const rec of result.defaultSample) {
    assert.equal(rec.metricFontSize, '22px', `${name}: ${rec.widgetId} default metric font-size must be 22px, got ${rec.metricFontSize}`);
    assert.equal(rec.metricFontWeight, '700', `${name}: ${rec.widgetId} default metric font-weight must be 700, got ${rec.metricFontWeight}`);
  }
  // 覆盖态：36px/400 全部传导（§9.0.1 字号/字重响应场景）。
  for (const rec of result.customSample) {
    assert.equal(rec.metricFontSize, '36px', `${name}: ${rec.widgetId} custom metric font-size must be 36px, got ${rec.metricFontSize}`);
    assert.equal(rec.metricFontWeight, '400', `${name}: ${rec.widgetId} custom metric font-weight must be 400, got ${rec.metricFontWeight}`);
  }

  await context.close();
  return { scenario: name, checked: result.customSample.length };
}

// §9.0.1 图形切换与多实例场景：成本卡 数字→表格→趋势 往返后统计同口径；
// 两张不同 range 的成本卡各自独立实例/设置，改其中一张不改变另一张。
// 数据层按 params.range 分发不同 overview——证明两张卡不共用同一查询结果。
async function runChartSwitchScenario(browser) {
  const name = 'chart-switch';
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error(`[${name} pageerror]`, String(e).slice(0, 300)));
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
     <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
     <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
  );
  writeFileSync(
    join(extRoot, 'space-verify-boot.mjs'),
    `import { createSpaceDashboard } from './space-dashboard.js';
     import { widgetPlugins } from './plugins/widgets/index.js';
     import { backgroundPlugins } from './plugins/backgrounds/index.js';
     window.__createDashboard = createSpaceDashboard;
     window.__widgetPlugins = widgetPlugins;
     window.__backgroundPlugins = backgroundPlugins;`
  );
  // 真实 Host 导出的 30d fixture + 构造的 7d 变体（数字可区分）；
  // analysis.byModel 两行使 table/bar 有 rank 数据（Host 契约字段）。
  const { readFileSync } = await import('node:fs');
  const hostFixture = JSON.parse(readFileSync(join(extRoot, 'ai-performance', 'host-fixture.json'), 'utf8'));
  const overview30 = hostFixture.overview;
  const overview7 = {
    ...overview30,
    metrics: { ...overview30.metrics, totalRequests: 5, estimatedCostUsd: 1.23 },
    tokens: { ...overview30.tokens, input: 420000, cacheRead: 100000 },
    trend: [{ timestamp: '2026-09-12T00:00:00Z', requests: 5, tokens: 520000, costUsd: 1.23 }],
  };
  const analysis = {
    status: 'ready',
    byModel: [
      { key: 'test-model-a', tokens: 1000000, requests: 2, costUsd: 0.28 },
      { key: 'test-model-b', tokens: 5200, requests: 1, costUsd: 0.02 },
    ],
    bySource: [{ key: 'claude-code', tokens: 1005200, requests: 3, costUsd: 0.3 }],
    byProvider: [],
    byHour: [],
  };
  writeFileSync(
    join(extRoot, 'space-verify-native-fixture.json'),
    JSON.stringify({ overview30, overview7, analysis, events: { events: [{ id: 'ev1', requestedAt: '2026-09-11T10:00:00Z', source: 'claude-code', tokens: 1000 }] } })
  );
  const bootHtml = readFileSync(join(extRoot, 'space-verify-boot.html'), 'utf8');
  writeFileSync(
    join(extRoot, 'space-verify-boot.html'),
    bootHtml.replace(
      '</body>',
      `<script>
        window.__nativeFixture = null;
        fetch('/space-verify-native-fixture.json').then((r) => r.json()).then((d) => { window.__nativeFixture = d; });
        window.chrome = {
          runtime: {
            connectNative: () => {
              const listeners = new Set();
              return {
                onMessage: { addListener: (fn) => listeners.add(fn) },
                onDisconnect: { addListener: () => {} },
                postMessage: (msg) => {
                  const respond = () => {
                    if (!window.__nativeFixture) return;
                    const f = window.__nativeFixture;
                    let result;
                    if (msg.method === 'model_usage_overview') {
                      result = msg.params && msg.params.range === '7d' ? f.overview7 : f.overview30;
                    } else if (msg.method === 'model_usage_analysis') {
                      result = f.analysis;
                    } else if (msg.method === 'model_usage_events') {
                      result = f.events;
                    } else {
                      result = { status: 'ready' };
                    }
                    for (const fn of [...listeners]) fn({ id: msg.id, ok: true, result });
                  };
                  if (window.__nativeFixture) respond();
                  else setTimeout(respond, 50);
                },
                disconnect: () => {},
              };
            },
          },
        };
      </script></body>`
    )
  );
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

  // 八张卡：七组件 + 第二张 aiCost（range=7d）——同 key 多实例（§4.1 可多次添加）。
  const snapshotForCharts = (costAChart, costBChart) => {
    const snap = snapshotFor({}, false, '#101210');
    snap.widgets[0].configJson = { metric: 'ai_cost', view: 'usage', chart: costAChart, range: '30d', dimension: 'model', subMetric: 'total' };
    snap.widgets.push({
      id: 'w-7',
      key: 'widget/aiCost',
      enabled: true,
      configJson: { metric: 'ai_cost', view: 'usage', chart: costBChart, range: '7d', dimension: 'model', subMetric: 'total' },
      displayJson: { position: 'free', xPercent: 12, yPercent: 70 },
    });
    return snap;
  };
  const sample = () =>
    page.evaluate(() => {
      const host = document.getElementById('dashboard-host');
      const out = {};
      for (const el of host.shadowRoot.querySelectorAll('[data-widget-id]:not(.Slot)')) {
        const id = el.dataset.widgetId;
        const num = el.querySelector('.ai-perf-number-value');
        const table = el.querySelector('.ai-perf-table-head');
        const svg = el.querySelector('.ai-perf-line-svg');
        const caption = el.querySelector('.ai-perf-chart-max');
        out[id] = {
          number: num ? num.textContent : null,
          tableHead: table ? table.textContent : null,
          hasLine: Boolean(svg),
          lineMax: caption ? caption.textContent : null,
        };
      }
      return out;
    });

  const rounds = [];
  const renderRound = async (label, chartA, chartB) => {
    await page.evaluate(({ snap }) => {
      const host = document.getElementById('dashboard-host');
      window.__dashboard.render(snap, 'ws-test', () => {});
    }, { snap: snapshotForCharts(chartA, chartB) });
    await new Promise((r) => setTimeout(r, 700));
    rounds.push({ label, state: await sample() });
  };

  // 暴露 dashboard 实例供轮次重渲染（首次 render 建立）。
  await page.evaluate(() => {
    const host = document.getElementById('dashboard-host');
    host.style.width = '1200px';
    host.style.height = '760px';
    host.style.background = '#101210';
    window.__dashboard = window.__createDashboard({
      $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
      t: (key, fallback) => fallback || key,
      selectedLanguage: 'zh_CN',
      backgroundPlugins: window.__backgroundPlugins,
      widgetPlugins: window.__widgetPlugins,
      nativeCall: async (method) => {
        throw new Error('unexpected dashboard nativeCall ' + method);
      },
      broadcastRevision: () => {},
      queueWorkspaceMutation: () => {},
      toast: () => {},
    });
  });
  await renderRound('both-number', 'number', 'number');
  await renderRound('a-table-b-number', 'table', 'number');
  await renderRound('a-line-b-number', 'line', 'number');
  await renderRound('a-number-again', 'number', 'number');

  const A = 'w-0';
  const B = 'w-7';
  // 数字态：两张成本卡显示各自 range 的估算金额（30d=$0.00，7d=$1.23）。
  assert.equal(rounds[0].state[A].number, '$0.00', `${name}: cost A (30d) number must be $0.00, got ${rounds[0].state[A].number}`);
  assert.equal(rounds[0].state[B].number, '$1.23', `${name}: cost B (7d) number must be $1.23, got ${rounds[0].state[B].number}`);
  // 表格态：成本列表头是费用列（按 metricId 定义，不硬编码 requests/tokens 两列）。
  assert.ok(rounds[1].state[A].tableHead && rounds[1].state[A].tableHead.includes('费用'), `${name}: cost A table must show cost column, got ${rounds[1].state[A].tableHead}`);
  // B 不受 A 切换影响（独立实例/设置）。
  assert.equal(rounds[1].state[B].number, '$1.23', `${name}: cost B must stay $1.23 while A switches, got ${rounds[1].state[B].number}`);
  // 趋势态：成本卡 line 的 max 标注用美元 formatter（同口径，不用 Token 格式）。
  assert.ok(rounds[2].state[A].hasLine, `${name}: cost A line chart must render`);
  assert.ok(rounds[2].state[A].lineMax && rounds[2].state[A].lineMax.startsWith('$'), `${name}: cost A line max must be USD-formatted, got ${rounds[2].state[A].lineMax}`);
  assert.equal(rounds[2].state[B].number, '$1.23', `${name}: cost B must stay $1.23 while A on line, got ${rounds[2].state[B].number}`);
  // 往返回数字：同口径同值，无旧状态残留。
  assert.equal(rounds[3].state[A].number, '$0.00', `${name}: cost A round-trip number must stay $0.00, got ${rounds[3].state[A].number}`);

  await context.close();
  return { scenario: name, rounds: rounds.length };
}

const artifactsDir = join(extRoot, 'tests', 'artifacts');
mkdirSync(artifactsDir, { recursive: true });

const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});

try {
  const dark = await runScenario(browser, 'space-dark', '#101210');
  const light = await runScenario(browser, 'space-light', '#f4f1e8');
  const reversed = await runScenario(
    browser,
    'global-light-space-dark-reverse',
    '#20242c',
    { useAccentColor: false }
  );
  // 显式覆盖场景：用户给相邻两卡设置不同 colour——只验证"保留与不串色"，
  // 不参与默认对比度断言（§5.2：显式颜色保留，低对比度由预览提示恢复）。
  const explicit = await runScenario(
    browser,
    'explicit-colours-dark',
    '#101210',
    {},
    true
  );
  // §5.5 动态切换：外观深→浅→深往返，零数据请求 + 派生色随背景更新。
  const toggle = await runAppearanceToggleScenario(browser);
  // §5.5 字体设置：主指标字号/字重响应，默认不被宿主字号传染。
  const fontSettings = await runFontSettingsScenario(browser);
  // §9.0.1 图形切换 + 双成本卡多实例互不影响。
  const chartSwitch = await runChartSwitchScenario(browser);

  writeFileSync(
    join(artifactsDir, 'space-ai-seven-widgets-verify.json'),
    JSON.stringify({ generatedAt: new Date().toISOString(), scenarios: [dark, light, reversed, explicit], appearanceToggle: toggle, fontSettings, chartSwitch }, null, 2)
  );
  console.log('SEVEN-WIDGET SPACE VERIFY PASSED');
  for (const s of [dark, light, reversed]) {
    console.log(`  ${s.scenario}: 7 widgets mounted, derived default contrast >= 4.5:1 against ${s.background}`);
  }
  console.log(`  ${explicit.scenario}: 7 widgets mounted, explicit per-card colours preserved`);
  console.log(`  ${toggle.scenario}: round-trip toggle, native calls ${toggle.callsBefore} -> ${toggle.callsAfter} (0 data requests)`);
  console.log(`  ${fontSettings.scenario}: metric font 22px/700 default -> 36px/400 override on ${fontSettings.checked} cards`);
  console.log(`  ${chartSwitch.scenario}: ${chartSwitch.rounds} rounds, cost A $0.00(30d)/B $1.23(7d) independent, table=cost col, line=USD fmt`);
} finally {
  // 清理 boot 文件（验证产物保留在 tests/artifacts/）。
  const { rmSync } = await import('node:fs');
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  server.close();
  await browser.close();
}
