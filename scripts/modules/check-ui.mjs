#!/usr/bin/env node
// 内置应用 UI 资源门禁（ADR-0031 / 架构 Wave 8 / 审查 A-11）：
// 验证每个模块在 registry.json 中的 UI 资源源码（src）与发布构建（dist）一致，
// 且 Rust ui.rs 的所有 include_str! 引用全部存在，无绝对路径泄漏。
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { fundLocalesSource, isFundModuleKey, FUND_MODULE_KEY_PREFIXES } from './fund-locales.mjs';
import { extensionFileBytes } from '../extension-package.mjs';
import { composeThemeCss, findDuplicatedBaseRoles } from './theme-compose.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const REGISTRY_PATH = join(ROOT, 'modules', 'registry.json');
const THEMED_MODULE_IDS = new Set(['fund', 'tokenusage']);
const BASE_THEME_PATH = join(ROOT, 'extension', 'theme.css');

const registry = JSON.parse(readFileSync(REGISTRY_PATH, 'utf8'));
for(const locale of ['zh_CN','en']) {
  const source=JSON.parse(readFileSync(join(ROOT,'extension/_locales',locale,'messages.json'),'utf8'));
  const packed=JSON.parse(extensionFileBytes(`_locales/${locale}/messages.json`,ROOT));
  if(Object.keys(packed).some(key=>isFundModuleKey(key)) || Object.keys(source).some(key=>!isFundModuleKey(key) && !packed[key]))throw Error('Extension locale distribution violates module ownership');
}
{
  // R-I2：模块命名空间的 zh/en key 集合必须完全一致（缺键回归入口）。
  const [zh,en]=['zh_CN','en'].map(locale=>new Set(Object.keys(JSON.parse(readFileSync(join(ROOT,'extension/_locales',locale,'messages.json'),'utf8'))).filter(isFundModuleKey)));
  const divergent=[...new Set([...zh,...en])].filter(key=>!zh.has(key)||!en.has(key));
  if(divergent.length)throw Error(`Fund module locale keys diverge between zh_CN and en: ${divergent.join(',')}`);
}
const fundLocaleTables=['zh_CN','en'].map(locale=>JSON.parse(readFileSync(join(ROOT,'extension/_locales',locale,'messages.json'),'utf8')));
if (readFileSync(join(ROOT, 'modules/fund/ui/src/home-i18n.js'), 'utf8') !== fundLocalesSource(ROOT)) {
  throw new Error('Fund locale projection is stale; run modules:ui:build');
}
{
  // 投影完整性（缺键回归入口）：locale 权威中的每个模块键都必须进入 home-i18n 投影。
  const projection=readFileSync(join(ROOT,'modules/fund/ui/dist/home-i18n.js'),'utf8');
  const missing=Object.keys(fundLocaleTables[0]).filter(key=>isFundModuleKey(key)&&!projection.includes(JSON.stringify(key)));
  if(missing.length)throw Error(`Fund locale projection missing keys: ${missing.slice(0,5).join(',')}`);
}

// 严格预期产物校验（DES 02 / A41）：dist 每个文件必须与 src 逐字节一致；
// 唯一例外 styles/theme.css 必须等于「基础主题＋模块主题」的确定性组合。
// 缺输入、陈旧产物、手改 dist、多余文件、重复声明基础角色都失败；
// 不维护排除名单，不放宽 tree 检查。
function verifyDistProjection(appId, srcDir, distDir) {
  const expectedFiles = new Map();
  const collect = (dir, prefix = '') => {
    for (const ent of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const rel = prefix ? `${prefix}/${ent.name}` : ent.name;
      if (ent.isDirectory()) collect(join(dir, ent.name), rel);
      else expectedFiles.set(rel.replaceAll('\\', '/'), join(dir, ent.name));
    }
  };
  collect(srcDir);

  if (!existsSync(distDir)) throw new Error(`${appId}: ui/dist missing; run modules:ui:build`);
  const actualFiles = new Map();
  const collectDist = (dir, prefix = '') => {
    for (const ent of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const rel = prefix ? `${prefix}/${ent.name}` : ent.name;
      if (ent.isDirectory()) collectDist(join(dir, ent.name), rel);
      else actualFiles.set(rel.replaceAll('\\', '/'), join(dir, ent.name));
    }
  };
  collectDist(distDir);

  for (const [rel, srcPath] of expectedFiles) {
    const distPath = actualFiles.get(rel);
    if (!distPath) throw new Error(`${appId}: ui/dist/${rel} missing; run modules:ui:build`);
    const expected = rel === 'styles/theme.css' && THEMED_MODULE_IDS.has(appId)
      ? composeThemeCss(readFileSync(BASE_THEME_PATH, 'utf8'), readFileSync(srcPath, 'utf8'))
      : readFileSync(srcPath, 'utf8');
    const actual = readFileSync(distPath, 'utf8');
    if (actual !== expected) {
      throw new Error(`${appId}: ui/dist/${rel} is stale or hand-edited (must be a byte-exact projection of ui/src${rel === 'styles/theme.css' && THEMED_MODULE_IDS.has(appId) ? ' + extension/theme.css composition' : ''}); run modules:ui:build`);
    }
  }
  for (const rel of actualFiles.keys()) {
    if (!expectedFiles.has(rel)) throw new Error(`${appId}: ui/dist/${rel} has no ui/src counterpart; dist must not carry extra files`);
  }

  if (THEMED_MODULE_IDS.has(appId)) {
    if (!existsSync(BASE_THEME_PATH)) throw new Error('extension/theme.css (single base theme source) is required');
    const moduleTheme = readFileSync(join(srcDir, 'styles', 'theme.css'), 'utf8');
    const duplicated = findDuplicatedBaseRoles(moduleTheme);
    if (duplicated.length) throw new Error(`${appId}: module theme redeclares base roles owned by extension/theme.css: ${duplicated.join(', ')}`);
  }
}

for (const app of registry.apps) {
  const appId = app.appId;
  const modDir = join(ROOT, 'modules', appId);
  const srcIndex = join(modDir, 'ui', 'src', 'index.html');
  const distIndex = join(modDir, 'ui', 'dist', 'index.html');

  if (!existsSync(srcIndex)) {
    console.error(`Error: ${appId} missing ui/src/index.html`);
    process.exit(1);
  }
  if (!existsSync(distIndex)) {
    console.error(`Error: ${appId} missing ui/dist/index.html`);
    process.exit(1);
  }
  try {
    verifyDistProjection(appId, join(modDir, 'ui', 'src'), join(modDir, 'ui', 'dist'));
  } catch (error) {
    console.error(`Error: ${error.message}`);
    process.exit(1);
  }

  // 检查 ui.rs 中的 include_str! 路径全部有效
  const uiRsPath = join(modDir, 'src', 'ui.rs');
  if (existsSync(uiRsPath)) {
    const uiRsText = readFileSync(uiRsPath, 'utf8');
    const matches = uiRsText.matchAll(/include_str!\(["']\.\.\/ui\/dist\/([^"']+)["']\)/g);
    for (const m of matches) {
      const subpath = m[1];
      const target = join(modDir, 'ui', 'dist', subpath);
      if (!existsSync(target)) {
        console.error(`Error: ${appId}/src/ui.rs includes missing file: ui/dist/${subpath}`);
        process.exit(1);
      }
    }
  }

  // 检查 dist 中无个人绝对路径泄漏
  function checkNoAbsolutePaths(dir) {
    for (const ent of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, ent.name);
      if (ent.isDirectory()) {
        checkNoAbsolutePaths(p);
      } else if (ent.isFile() && /\.(js|html|css|json)$/.test(ent.name)) {
        if(ent.name.endsWith('.js'))execFileSync(process.execPath,['--check',p],{stdio:'pipe'});
        const content = readFileSync(p, 'utf8');
        if(appId==='fund'&&ent.name!=='home-i18n.js'){
          const moduleTokenRe=new RegExp(`(?:${FUND_MODULE_KEY_PREFIXES.join('|')})[A-Za-z0-9_]+`,'g');
          for(const key of new Set(content.match(moduleTokenRe)||[])){
            if(fundLocaleTables.some(table=>!table[key]))throw Error(`Missing Fund locale key: ${key}`);
          }
        }
        // 绝对路径泄漏检测：仅匹配以 /Users/<name> 或 /home/<name> 开头的真实
        // 本机路径形态；不把 /api/home/...、/home/ 这类站内路由当泄漏（07 R10）。
        const leakPattern = /(?<!["'\/\w])\/(?:Users|home)\/[A-Za-z0-9._-]+(?:\/[^\s"'`<>]*)?/;
        const m = content.match(leakPattern);
        if (m) {
          console.error(`Error: ${p} contains local absolute path: ${m[0]}`);
          process.exit(1);
        }
      }
    }
  }
  checkNoAbsolutePaths(join(modDir, 'ui', 'dist'));
}

console.log(`✓ UI asset verification passed for all registered modules: ${registry.apps.map(a => a.appId).join(', ')}`);
