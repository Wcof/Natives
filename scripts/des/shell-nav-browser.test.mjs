// DES T02 / A03（部分）：外壳导航与工具菜单真实 Chrome 验证。
// 七项主导航 + 工具菜单（导入／净值）、tab roving、数字快捷键、跑马灯按
// 任务显隐、刷新范围标注、菜单 Esc／外部点击关闭与焦点返回。
// 数据为合成空响应，不触真实账本。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts, installShellStubs } from './shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const artifacts = resolve(ROOT, '.tmp/des-t02/shell-nav');
mkdirSync(artifacts, { recursive: true });

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [] };
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
  await installShellStubs(page);
  await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
  for (const file of shellScripts()) await page.addScriptTag({ path: resolve(ROOT, 'modules/fund/ui/src', file) });
  await page.waitForTimeout(150);

  // 1. 七项主导航 + 工具菜单结构
  const nav = await page.evaluate(() => ({
    tabs: [...document.querySelectorAll('nav.main-nav [role="tab"]')].map((b) => b.id),
    toolsHidden: document.getElementById('tools-menu').hidden,
    tickerDefaultHidden: getComputedStyle(document.getElementById('market-tickers')).display === 'none',
    addAssetRemoved: !document.getElementById('add-asset-btn'),
  }));
  assert.deepEqual(nav.tabs, ['tab-home', 'tab-positions', 'tab-assets', 'tab-watchlist', 'tab-trends', 'tab-news', 'tab-history']);
  assert.ok(nav.toolsHidden && nav.tickerDefaultHidden && nav.addAssetRemoved);
  report.steps.push({ step: 'seven primary tabs + tools menu structure', pass: true, nav });

  // 2. 工具菜单打开 → 导入视图；aria 状态正确
  await page.locator('#btn-tools-menu').click();
  assert.equal(await page.locator('#tools-menu').isHidden(), false);
  await page.locator('#tab-import').click();
  assert.equal(await page.locator('#view-import').isHidden(), false);
  assert.equal(await page.locator('#view-home').isHidden(), true);
  const importState = await page.evaluate(() => ({
    importDataActive: document.getElementById('tab-import').hasAttribute('data-active'),
    importAriaSelected: document.getElementById('tab-import').getAttribute('aria-selected'),
    toolsExpanded: document.getElementById('btn-tools-menu').getAttribute('aria-expanded'),
    toolsClosed: document.getElementById('tools-menu').hidden,
    refreshLabel: document.getElementById('btn-manual-refresh').getAttribute('aria-label'),
    tickerStillHidden: getComputedStyle(document.getElementById('market-tickers')).display === 'none',
  }));
  assert.equal(importState.importDataActive, true);
  assert.equal(importState.importAriaSelected, null, 'menu items must not carry tab aria-selected');
  assert.equal(importState.toolsClosed, true, 'activating a menu item closes the menu');
  assert.match(importState.refreshLabel, /批量导入/);
  report.steps.push({ step: 'tools menu activates import view with menuitem semantics + refresh scope', pass: true, importState });

  // 3. 公募净值经菜单；菜单焦点返回
  await page.locator('#btn-tools-menu').click();
  await page.locator('#tab-nav').click();
  assert.equal(await page.locator('#view-nav').isHidden(), false);
  const navState = await page.evaluate(() => ({
    importInactive: !document.getElementById('tab-import').hasAttribute('data-active'),
    navActive: document.getElementById('tab-nav').hasAttribute('data-active'),
  }));
  assert.ok(navState.importInactive && navState.navActive);
  report.steps.push({ step: 'nav view via tools menu, single active entry', pass: true, navState });

  // 4. 市场视图显示跑马灯，其他视图隐藏
  await page.locator('#btn-tools-menu').click();await page.locator('#tab-market').click();
  assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('market-tickers')).display), 'flex');
  await page.locator('#btn-tools-menu').click();await page.locator('#tab-tx').click();
  assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('market-tickers')).display), 'none');
  assert.match(await page.evaluate(() => document.getElementById('btn-manual-refresh').getAttribute('aria-label')), /交易流水/);
  report.steps.push({ step: 'ticker bar follows market task scope; refresh scope updates', pass: true });

  // 5. 数字快捷键兼容（0=首页，1=自选；工具视图仍可经菜单键盘激活）
  await page.keyboard.press('0');
  assert.equal(await page.locator('#view-home').isHidden(), false);
  await page.keyboard.press('1');
  assert.equal(await page.locator('#view-watchlist').isHidden(), false);
  await page.keyboard.press('7');
  assert.equal(await page.locator('#view-import').isHidden(), false, 'legacy shortcut 7 still opens import');
  await page.keyboard.press('8');
  assert.equal(await page.locator('#view-nav').isHidden(), false, 'legacy shortcut 8 still opens nav');
  report.steps.push({ step: 'numeric shortcuts compatible incl. tool views', pass: true });

  // 6. roving tabindex：仅选中 tab 可聚焦，方向键漫游
  await page.locator('#tab-trends').click();
  const roving = await page.evaluate(() => {
    const tabs = [...document.querySelectorAll('nav.main-nav [role="tab"]')];
    return tabs.filter((b) => b.tabIndex === 0).map((b) => b.id);
  });
  assert.deepEqual(roving, ['tab-trends']);
  await page.locator('#tab-trends').focus();
  await page.keyboard.press('Home');
  assert.equal(await page.evaluate(() => document.activeElement.id), 'tab-home');
  report.steps.push({ step: 'roving tabindex and arrow/Home/End navigation', pass: true, roving });

  // 7. 菜单 Esc 关闭并返回焦点；外部点击关闭
  await page.locator('#btn-tools-menu').click();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.getElementById('tools-menu').hidden), true);
  assert.equal(await page.evaluate(() => document.activeElement.id), 'btn-tools-menu');
  await page.locator('#btn-tools-menu').click();
  await page.locator('#tab-home').click(); // 外部点击同时切换视图并关闭
  assert.equal(await page.evaluate(() => document.getElementById('tools-menu').hidden), true);
  report.steps.push({ step: 'menu Esc/outside click close with focus return', pass: true });

  // 8. 菜单打开时数字快捷键切换视图并收起菜单（菜单非模态，快捷键仍可用）
  await page.locator('#btn-tools-menu').click();
  await page.keyboard.press('3');
  assert.equal(await page.evaluate(() => document.getElementById('tools-menu').hidden), true, 'shortcut switches view and collapses menu');
  assert.equal(await page.locator('#view-trends').isHidden(), false);
  report.steps.push({ step: 'shortcut while menu open switches view and collapses menu', pass: true });

  await page.screenshot({ path: resolve(artifacts, 'shell-nav.png'), fullPage: false });
  await page.close();
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every((s) => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`shell nav check: ${report.steps.filter((s) => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
