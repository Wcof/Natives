#!/usr/bin/env node
// 内置子应用 UI 构建脚本（ADR-0031 / 架构 Wave 8 / DES 02 单源主题）：
// 将 modules/<appId>/ui/src 中的前端资源确定性同步/构建至 ui/dist/。
// styles/theme.css 例外：由「产品基础主题 extension/theme.css（唯一可编辑
// 源）＋模块 src/styles/theme.css（金融/图表/密度角色）」在构建期确定性
// 组合生成；组合后再计算 tree 摘要，check-ui 按同一组合严格校验字节。
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, cpSync, rmSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { fundLocalesSource } from './fund-locales.mjs';
import { composeThemeCss, readBaseThemeCss } from './theme-compose.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const REGISTRY_PATH = join(ROOT, 'modules', 'registry.json');

const registry = JSON.parse(readFileSync(REGISTRY_PATH, 'utf8'));

// 单源组合规则（DES 02 / DES-D07）：基础主题在前，模块主题在后；
// 模块主题可按 data-theme 覆盖/补充金融语义，但不得重复声明基础角色
// （由 check-ui 校验）。除该文件外，dist 仍是 src 的逐字节投影。
const THEMED_MODULE_IDS = new Set(['fund', 'tokenusage']);

function computeTreeHash(dir) {
  const hash = createHash('sha256');
  function walk(d) {
    const entries = readdirSync(d, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
    for (const ent of entries) {
      const full = join(d, ent.name);
      if (ent.isDirectory()) {
        walk(full);
      } else if (ent.isFile()) {
        hash.update(ent.name);
        hash.update(readFileSync(full));
      }
    }
  }
  walk(dir);
  return hash.digest('hex');
}

console.log('Building module UI assets...');
writeFileSync(join(ROOT, 'modules/fund/ui/src/home-i18n.js'), fundLocalesSource(ROOT));

const baseThemeCss = readBaseThemeCss(ROOT);

for (const app of registry.apps) {
  const appId = app.appId;
  const uiDir = join(ROOT, 'modules', appId, 'ui');
  const srcDir = join(uiDir, 'src');
  const distDir = join(uiDir, 'dist');

  if (!existsSync(srcDir)) {
    console.error(`Error: ${appId} missing ui/src directory at ${srcDir}`);
    process.exit(1);
  }

  // Remove stale assets so the output depends only on ui/src (+ shared base theme).
  rmSync(distDir, { recursive: true, force: true });
  mkdirSync(distDir, { recursive: true });

  // 递归同步 src 到 dist
  cpSync(srcDir, distDir, { recursive: true, force: true });

  // 单源主题组合：覆盖 dist/styles/theme.css 后再计算摘要。
  if (THEMED_MODULE_IDS.has(appId)) {
    const moduleThemePath = join(srcDir, 'styles', 'theme.css');
    if (!existsSync(moduleThemePath)) {
      console.error(`Error: ${appId} expects module theme input at ui/src/styles/theme.css`);
      process.exit(1);
    }
    writeFileSync(join(distDir, 'styles', 'theme.css'), composeThemeCss(baseThemeCss, readFileSync(moduleThemePath, 'utf8')));
  }

  const treeSha = computeTreeHash(distDir);
  console.log(`  ✓ Built ${appId} UI: ${distDir} (treeSha256: ${treeSha.slice(0, 16)}...)`);
}

console.log('All module UI assets built successfully.');
