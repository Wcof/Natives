import { test } from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import {
  MOTION_EASING,
  TRANSITION,
  SPINNER_EASING,
  LAYOUT,
  NEUTRAL_PALETTE,
  CHART_VOLUME_DARK,
  CHART_VOLUME_LIGHT,
  V2_TOKENS,
  V2_THEME_NAMES,
  TERMINAL_THEMES,
  THEMES,
  type V2ThemeId,
  type V2ThemeTokens,
} from './design-tokens';

// ── Foundation Motion / Layout Token Contract Tests ──
// R-U13/R-U14: 统一缓动曲线 + 三档时长；spinner 是唯一 linear 例外。
// R-U2.10: 共享结构令牌（阅读宽度、标题栏、紧凑控件、回合间距）。

test('MOTION_EASING uses the single unified curve', () => {
  assert.equal(MOTION_EASING, 'cubic-bezier(0.16, 1, 0.3, 1)');
});

test('TRANSITION tiers all reference MOTION_EASING', () => {
  for (const tier of [TRANSITION.fast, TRANSITION.normal, TRANSITION.slow]) {
    assert.ok(tier.endsWith(MOTION_EASING), `${tier} should end with ${MOTION_EASING}`);
  }
});

test('TRANSITION tiers are 120/200/300ms', () => {
  assert.equal(TRANSITION.fast, `120ms ${MOTION_EASING}`);
  assert.equal(TRANSITION.normal, `200ms ${MOTION_EASING}`);
  assert.equal(TRANSITION.slow, `300ms ${MOTION_EASING}`);
});

test('SPINNER_EASING is the only linear exception (R-U13)', () => {
  assert.equal(SPINNER_EASING, 'linear');
});

test('shared layout structure tokens (R-U2.10)', () => {
  assert.equal(LAYOUT.readingWidth, 760);
  assert.equal(LAYOUT.titlebarHeight, 48);
  assert.equal(LAYOUT.controlCompact, 32);
  assert.equal(LAYOUT.turnGap, 40);
});

// ── 15 级全局中性色板（R-U2.5）：固定值，深浅主题同值 ──
const NEUTRAL_EXPECTED: Record<string, string> = {
  '0': '#010101',
  '100': '#111113',
  '150': '#18181A',
  '200': '#202024',
  '250': '#27262B',
  '300': '#323137',
  '400': '#48474D',
  '500': '#646268',
  '600': '#7F7D83',
  '700': '#9B999E',
  '800': '#B7B5BA',
  '850': '#D4D3D7',
  '900': '#E8E7EA',
  '950': '#FAFAFC',
  '1000': '#FFFFFF',
};

test('NEUTRAL_PALETTE is the fixed 15-level global spectrum (R-U2.5)', () => {
  const entries = Object.entries(NEUTRAL_PALETTE);
  assert.equal(entries.length, Object.keys(NEUTRAL_EXPECTED).length);
  for (const [step, hex] of Object.entries(NEUTRAL_EXPECTED)) {
    assert.equal(NEUTRAL_PALETTE[Number(step) as keyof typeof NEUTRAL_PALETTE], hex, `neutral-${step}`);
  }
});

// ═══════════════════════════════════════════
// V2 双主题：键集合 100% 对齐 + 颜色格式合法、无缺失字段
// ═══════════════════════════════════════════

const V2_KEYS = Object.keys(V2_TOKENS.dark) as Array<keyof V2ThemeTokens>;

const HEX6_RE = /^#[0-9a-fA-F]{6}$/;
const HEX8_RE = /^#[0-9a-fA-F]{8}$/;
const RGB_RE = /^rgba?\([^)]*[\d.%][^)]*\)$/;

const COLOR_TOKEN_RE =
  /(#[0-9a-fA-F]{6}|#[0-9a-fA-F]{8}|rgba?\([^)]*\)|color-mix\([^)]*\)|var\(--[a-z0-9-]+\))/;

/** CSS 尺寸/关键字（阴影的骨架部分）：如 0, 1px, 2px, 8px 24px 等 */
const SIZE_TOKEN_RE = /^[\d.]+(px|em|rem|vh|vw|%|mm|cm)?$/;

/**
 * 校验阴影值（box-shadow 与 glow 均为阴影列表）：
 * 结构 = [inset?] <尺寸>* <颜色> [, 另一层]*，其中每层至少一个颜色 token。
 */
/** 在括号之外按顶层逗号切分（阴影层：rgba 内部含逗号，不应切分）。 */
function splitTopLevelCommas(value: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of value) {
    if (ch === '(') depth += 1;
    if (ch === ')') depth = Math.max(0, depth - 1);
    if (ch === ',' && depth === 0) {
      parts.push(current.trim());
      current = '';
    } else {
      current += ch;
    }
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}

function isBoxShadowValue(value: string): boolean {
  const v = value.trim();
  if (v === 'none') return true;
  const layers = splitTopLevelCommas(v);
  return layers.length > 0 && layers.every(isSingleBoxShadowLayer);
}

function isSingleBoxShadowLayer(layer: string): boolean {
  if (!layer) return false;
  // 先把颜色函数整体替换为哨兵，防止 rgba(0, 0, 0, ...) 内部的空格干扰按空白切分
  const stripped = layer.replace(COLOR_TOKEN_RE, ' COLOR ');
  let hasColor = /COLOR/.test(stripped);
  const tokens = stripped.trim().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return false;
  let hasSize = false;
  for (const tok of tokens) {
    if (tok === 'inset') continue;
    if (tok === 'COLOR') {
      hasColor = true;
      continue;
    }
    if (SIZE_TOKEN_RE.test(tok)) {
      hasSize = true;
      continue;
    }
    // 未知 token → 非法
    return false;
  }
  // 至少一个尺寸且至少一个颜色分量
  return hasSize && hasColor;
}

/**
 * 校验一个语义 token 值是否是合法 CSS 颜色/渐变/阴影。
 * 允许：hex6 / hex8 / rgb() / rgba() / color-mix() / var(--x) / linear-gradient() /
 *       box-shadow 值（每层含尺寸 + 颜色） / none。
 */
function isValidCssValue(value: string): boolean {
  const v = value.trim();
  if (!v) return false;
  if (HEX6_RE.test(v) || HEX8_RE.test(v)) return true;
  if (RGB_RE.test(v)) return true;
  if (/^var\(--[a-z0-9-]+\)$/.test(v)) return true;
  if (v === 'none') return true;
  if (/^color-mix\([\s\S]+\)$/.test(v)) return true;
  if (/^linear-gradient\([\s\S]+\)$/.test(v)) return true;
  if (/^radial-gradient\([\s\S]+\)$/.test(v)) return true;
  return isBoxShadowValue(v);
}

const NEUTRAL_AS_RECORD = NEUTRAL_PALETTE as unknown as Record<number, string>;

/** 解析 NEUTRAL_PALETTE 引用为 hex（用于与 CSS 比对）；其他值原样返回。 */
function resolveNeutral(value: string): string {
  const m = value.match(/NEUTRAL_PALETTE\[(\d+)\]/);
  if (!m) return value;
  return NEUTRAL_AS_RECORD[Number(m[1])] ?? value;
}

test('V2 dark/light 键集合 100% 对齐（无缺失、无多余）', () => {
  const darkKeys = Object.keys(V2_TOKENS.dark).sort();
  const lightKeys = Object.keys(V2_TOKENS.light).sort();
  assert.deepEqual(darkKeys, lightKeys, 'dark 与 light 键集合必须完全相同');
  assert.ok(darkKeys.length >= 104, `V2 语义 token 至少 104 键（当前 ${darkKeys.length}）`);
});

test('V2 主题没有缺失字段（每个键都非空字符串）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of V2_KEYS) {
      const value = V2_TOKENS[theme][key];
      assert.ok(
        typeof value === 'string' && value.trim().length > 0,
        `${theme}.${key} 不应为空`,
      );
    }
  }
});

test('所有颜色值符合 hex / rgba / 渐变 / 阴影 / var 合法格式', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of V2_KEYS) {
      const value = V2_TOKENS[theme][key];
      assert.ok(isValidCssValue(value), `${theme}.${key} 非法格式: "${value}"`);
    }
  }
});

test('var() 引用只允许同主题内部别名（canvas / text-ghost / 阴影引用）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of V2_KEYS) {
      const v = V2_TOKENS[theme][key];
      if (v.startsWith('var(--')) {
        const refs = v.match(/var\(--[a-z0-9-]+\)/g) ?? [];
        for (const ref of refs) {
          const name = ref.slice(4, -1); // --xxx（去掉前缀 var( 后缀 )）
          assert.ok(
            key === 'canvas' ||
              name === '--background' ||
              name === '--text-disabled' ||
              name === '--primary',
            `${theme}.${key} 引用未登记的内部变量 ${ref}`,
          );
        }
      }
    }
  }
});

// ═══════════════════════════════════════════
// ADR-0022 锚点值 + Design System V2 语义色阶
// ═══════════════════════════════════════════

test('Liquid Crystal 浅色：ADR-0022 画布/表面/inset/accent 锚点', () => {
  const light = V2_TOKENS.light;
  assert.equal(light.background, '#F5F6F8'); // 晶透液态画布 #F5F6F8~#F8FAFC
  assert.equal(light.surface, '#FFFFFF'); // 白玉玻璃表面 72% alpha
  assert.equal(light.inset, '#F1F3F5'); // 浅色 inset（ADR-0022 = #F1F3F5）
  assert.equal(light['surface-hover'], '#F1F3F5');
  assert.equal(light.accent, '#F97316'); // 珊瑚橙 accent 上界
  assert.equal(light['accent-hover'], '#EA580C'); // 珊瑚橙 accent 下界
});

test('Dark Glow 深色：ADR-0022 低反射深色 canvas / 黑晶表面', () => {
  const dark = V2_TOKENS.dark;
  assert.equal(dark.background, '#0D1117'); // 暗黑流光画布 #0B0F14~#0D1117
  assert.equal(dark.surface, '#121820'); // 88% 黑晶表面
  assert.equal(dark.inset, '#18202B'); // 暗色 inset（ADR-0022 = #18202B）
  assert.equal(dark.accent, '#F59E0B'); // 琥珀金 accent 下界
  assert.equal(dark['accent-hover'], '#FB923C'); // 琥珀金 accent 上界
});

test('R-U2.6 语义映射：raised/inset/composer 遵循固定映射', () => {
  assert.equal(V2_TOKENS.dark.raised, '#18202B', 'dark raised = surface-hover');
  assert.equal(V2_TOKENS.dark.inset, '#18202B', 'dark inset = surface-hover (ADR-0022)');
  assert.equal(V2_TOKENS.dark.composer, '#18202B', 'dark composer = surface-hover');
  assert.equal(V2_TOKENS.light.raised, '#FFFFFF', 'light raised = surface');
  assert.equal(V2_TOKENS.light.inset, '#F1F3F5', 'light inset = surface-hover (ADR-0022)');
  assert.equal(V2_TOKENS.light.composer, '#FFFFFF', 'light composer = surface');
});

test('chart volume 色阶 0–8 完整且方向正确（R-U2.7 表）', () => {
  const DARK_EXPECTED = ['#202024', '#323137', '#48474D', '#646268', '#7F7D83', '#9B999E', '#B7B5BA', '#D4D3D7', '#FAFAFC'];
  const LIGHT_EXPECTED = ['#E8E7EA', '#D4D3D7', '#B7B5BA', '#9B999E', '#7F7D83', '#646268', '#48474D', '#323137', '#18181A'];
  for (let i = 0; i <= 8; i++) {
    assert.equal(CHART_VOLUME_DARK[i], DARK_EXPECTED[i], `dark chart-volume-${i}`);
    assert.equal(CHART_VOLUME_LIGHT[i], LIGHT_EXPECTED[i], `light chart-volume-${i}`);
  }
});

test('状态呼吸圆点 4 态齐全（运行绿/停止灰/预警黄/故障红）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of ['status-running', 'status-stopped', 'status-warning', 'status-danger'] as const) {
      assert.ok(HEX6_RE.test(V2_TOKENS[theme][key]), `${theme}.${key}`);
    }
  }
});

test('chart 多维序列色 与 终端 ANSI 四要素联动（R-U4）', () => {
  assert.ok(HEX6_RE.test(V2_TOKENS.dark['chart-line']));
  assert.ok(HEX6_RE.test(V2_TOKENS.light['chart-line']));
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    const tt = TERMINAL_THEMES[theme];
    assert.ok(tt && tt.background && tt.foreground && tt.cursor, `${theme} 终端必须提供 bg/fg/cursor`);
    assert.ok(tt.selectionBackground, `${theme} 终端必须提供 selectionBackground`);
  }
});

test('Elevation surfaces 与 shadow 完整（四级 + 四个阴影）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of ['elevation-base', 'elevation-raised', 'elevation-floating', 'elevation-inset'] as const) {
      assert.ok(typeof V2_TOKENS[theme][key] === 'string' && V2_TOKENS[theme][key].length > 0, `${theme}.${key}`);
    }
    for (const key of ['elev-shadow-base', 'elev-shadow-raised', 'elev-shadow-floating', 'elev-shadow-inset'] as const) {
      assert.ok(isValidCssValue(V2_TOKENS[theme][key]), `${theme}.${key}`);
    }
  }
});

// ═══════════════════════════════════════════
// CSS mirror fallback 对齐（防漂移）
// ═══════════════════════════════════════════

const CSS_FILE_URL = new URL('../app/styles/tokens.css', import.meta.url);
const CSS_TEXT = readFileSync(CSS_FILE_URL, 'utf8');

function extractCssTokenMap(block: string): Record<string, string> {
  const map: Record<string, string> = {};
  const re = /^\s*--([a-z0-9-]+):\s*([^;\n]+);/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(block)) !== null) {
    const key = m[1] ?? '';
    const value = m[2] ?? '';
    if (key) map[key] = value.trim();
  }
  return map;
}

function resolveCssVar(map: Record<string, string>, value: string, depth = 0): string {
  if (depth > 10) return value;
  let v = value;
  for (let i = 0; i < 10; i++) {
    const pure = v.match(/^var\(--([a-z0-9-]+)\)$/);
    if (!pure) break;
    const ref = pure[1] ?? '';
    v = ref && map[ref] ? map[ref] : v;
  }
  return v.replace(/var\(--([a-z0-9-]+)\)/g, (_, k) => (k ? map[k] ?? `var(--${k})` : _));
}

const CSS_DARK_START = CSS_TEXT.indexOf('[data-theme="dark"] {');
const CSS_LIGHT_START = CSS_TEXT.indexOf('[data-theme="light"] {');
const CSS_DARK_MAP = extractCssTokenMap(CSS_TEXT.slice(CSS_DARK_START, CSS_LIGHT_START));
const CSS_LIGHT_MAP = extractCssTokenMap(
  CSS_TEXT.slice(CSS_LIGHT_START, CSS_TEXT.indexOf('/* ═══', CSS_LIGHT_START + 40)),
);

test('CSS mirror：每个 V2 语义键在深浅块都有 --key（无缺失字段）', () => {
  for (const key of V2_KEYS) {
    assert.ok(key in CSS_DARK_MAP, `dark css 缺少 --${key}`);
    assert.ok(key in CSS_LIGHT_MAP, `light css 缺少 --${key}`);
  }
});

test('CSS mirror：深浅主题解析后的 hex 与 V2_TOKENS 完全一致（0 漂移）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    const cssMap = theme === 'dark' ? CSS_DARK_MAP : CSS_LIGHT_MAP;
    for (const key of V2_KEYS) {
      if (key === 'canvas') continue; // canvas = var(--background) 双向引用，跳过
      const tsValue = resolveNeutral(V2_TOKENS[theme][key]);
      const cssValue = cssMap[key];
      if (!cssValue) continue;
      const cssResolved = resolveCssVar(cssMap, cssValue);
      const tsHex = tsValue.match(/#[0-9a-fA-F]{6}/)?.[0]?.toLowerCase();
      const cssHex = cssResolved.match(/#[0-9a-fA-F]{6}/)?.[0]?.toLowerCase();
      if (tsHex && cssHex) {
        assert.equal(
          cssHex,
          tsHex,
          `${theme}.${key} CSS mirror 与 V2_TOKENS 漂移: ts=${tsValue} css-resolved=${cssResolved}`,
        );
      }
    }
  }
});

// ═══════════════════════════════════════════
// 主题 ID / 显示名 / 向后兼容 契约
// ═══════════════════════════════════════════

test('V2_THEME_NAMES 使用 i18n 键（settings.*）', () => {
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    assert.match(V2_THEME_NAMES[theme].labelKey, /^settings\./);
    assert.match(V2_THEME_NAMES[theme].descKey, /^settings\./);
  }
});

test('THEMES 向后兼容子集派生自 V2_TOKENS', () => {
  assert.equal(THEMES.dark.background, V2_TOKENS.dark.background);
  assert.equal(THEMES.light.background, V2_TOKENS.light.background);
  assert.equal(THEMES.dark.surface, V2_TOKENS.dark.surface);
  assert.equal(THEMES.light.surface, V2_TOKENS.light.surface);
  assert.equal(THEMES.dark.text, V2_TOKENS.dark.text);
});

test('R-U2 必需语义键集合在此两主题中全部存在', () => {
  const required: Array<keyof V2ThemeTokens> = [
    'background', 'surface', 'surface-hover', 'sidebar', 'border', 'border-subtle',
    'text', 'text-body', 'text-secondary', 'text-disabled', 'primary', 'primary-hover',
    'primary-soft', 'primary-dark', 'accent', 'accent-hover', 'inset', 'raised', 'composer',
    'control-bg', 'control-bg-hover', 'control-fg', 'control-selected-bg', 'control-selected-bg-hover',
    'control-selected-fg', 'danger', 'warning', 'info', 'success', 'status-running',
    'status-stopped', 'status-warning', 'status-danger', 'chart-token-line', 'chart-requests-line',
    'chart-line', 'terminal-bg', 'terminal-fg', 'terminal-cursor', 'terminal-selection',
  ];
  for (const theme of ['dark', 'light'] as V2ThemeId[]) {
    for (const key of required) {
      assert.ok(key in V2_TOKENS[theme], `${theme} 缺少必需语义键 ${key}`);
    }
  }
});