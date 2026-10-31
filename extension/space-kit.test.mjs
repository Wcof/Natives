import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { setupTestDomEnvironment } from './test-dom-mock.js';

setupTestDomEnvironment();

const {
  CARD_CATEGORIES,
  CARD_META,
  WIDGET_KEYS,
  cardMeta,
  cardsInCategory,
  catalogWidgetKeys,
  normalizeCardLimit,
  dayKey,
  daysBetween,
  lastNDays,
  timeProgress,
  dailyIndex,
  zonedClock,
  pruneDayMap,
  formatStamp,
  createCardShell,
  createInlineForm,
  createListRow,
} = await import('./space-kit/index.js');
const { widgetPlugins } = await import('./space-plugins.js');

console.log('--- space-kit metadata & pure helpers ---');

// 1. 元数据完整性
const categoryIds = new Set(CARD_CATEGORIES.map((category) => category.id));
assert.equal(WIDGET_KEYS.length, 44, 'space-kit must keep the existing 37 keys and add the 7 new cards');
assert.equal(new Set(WIDGET_KEYS).size, WIDGET_KEYS.length, 'widget keys must be unique');
for (const key of WIDGET_KEYS) {
  const meta = CARD_META[key];
  assert.ok(meta, `missing metadata for ${key}`);
  assert.ok(categoryIds.has(meta.category), `unknown category for ${key}: ${meta.category}`);
  assert.ok(widgetPlugins[key], `plugin missing for metadata key ${key}`);
  assert.equal(widgetPlugins[key].key, key, `plugin key mismatch: ${key}`);
}
for (const key of Object.keys(widgetPlugins)) {
  assert.ok(cardMeta(key), `plugin ${key} is not declared in card-meta.js`);
}

const PUBLIC_KEYS = catalogWidgetKeys();
assert.ok(!PUBLIC_KEYS.includes('widget/aiPerformance'), 'legacy aiPerformance must stay hidden in the catalog');
assert.equal(
  PUBLIC_KEYS.length,
  WIDGET_KEYS.length - 1,
  'only the legacy aiPerformance key is hidden from the catalog',
);

const NEW_KEYS = [
  'widget/dailyFocus',
  'widget/quickNote',
  'widget/scratchpad',
  'widget/habit',
  'widget/timeProgress',
  'widget/worldClock',
  'widget/dailyQuestion',
];
for (const key of NEW_KEYS) {
  const meta = cardMeta(key);
  assert.ok(meta?.name && meta?.nameZh, `${key} must declare bilingual names`);
  assert.ok(meta?.description && meta?.descriptionZh, `${key} must declare bilingual descriptions`);
  assert.ok(PUBLIC_KEYS.includes(key), `${key} must appear in the catalog`);
  assert.ok(cardsInCategory(meta.category).includes(key), `${key} must be listed in its category`);
}

// 2. Rust Host 白名单同步（双注册表的可执行守卫）
const here = dirname(fileURLToPath(import.meta.url));
const schemaPath = join(here, '..', 'crates', 'native-file-host', 'src', 'workspace_store', 'schema.rs');
const schema = readFileSync(schemaPath, 'utf8');
for (const key of WIDGET_KEYS) {
  assert.ok(schema.includes(`"${key}"`), `Rust WIDGET_KEYS must contain ${key}`);
}

// 3. 通用 limit 夹取
assert.equal(normalizeCardLimit('widget/dailyFocus', 0), 1);
assert.equal(normalizeCardLimit('widget/dailyFocus', 99), 6);
assert.equal(normalizeCardLimit('widget/dailyFocus', 'abc'), 3);
assert.equal(normalizeCardLimit('widget/quickNote', 10), 10);
assert.equal(normalizeCardLimit('widget/time', 99), undefined);

// 4. 日期与进度
assert.equal(dayKey(new Date(2026, 9, 31, 12)), '2026-10-31');
assert.equal(daysBetween('2026-10-31', '2026-11-02'), 2);
assert.equal(daysBetween('bad', '2026-11-02'), null);
assert.deepEqual(lastNDays(3, new Date(2026, 9, 31, 12)), ['2026-10-29', '2026-10-30', '2026-10-31']);

const progress = timeProgress(new Date(2026, 9, 31, 12));
assert.ok(progress.day > 0.49 && progress.day < 0.51, 'noon should be about half of the day');
for (const value of Object.values(progress)) {
  assert.ok(value >= 0 && value <= 1, 'progress values must stay within 0..1');
}
assert.ok(progress.year > 0.8 && progress.year < 0.85, 'October 31 should be about 83% of the year');

// 5. 每日稳定选择
const indexA = dailyIndex('2026-10-31', 12);
const indexB = dailyIndex('2026-10-31', 12);
assert.equal(indexA, indexB, 'dailyIndex must be stable for the same day');
assert.ok(indexA >= 0 && indexA < 12);
assert.notEqual(dailyIndex('2026-10-31', 12), dailyIndex('2026-11-01', 12), 'different days should usually differ');
assert.equal(dailyIndex('2026-10-31', 0), -1);

// 6. 时区
const shanghai = zonedClock(new Date('2026-10-31T04:30:00Z'), 'Asia/Shanghai');
assert.equal(shanghai.time, '12:30');
assert.ok(Math.abs(shanghai.offset) <= 1, 'timezone offset must stay within one day');

// 7. 日键映射裁剪
const pruned = pruneDayMap({
  '2026-10-31': 1,
  '2026-10-30': 2,
  '2026-09-01': 3,
  bad: 4,
}, 7, '2026-10-31');
assert.deepEqual(pruned, { '2026-10-31': 1, '2026-10-30': 2 });

assert.equal(formatStamp(new Date(2026, 0, 5, 9, 4).getTime()), '1/5 09:04');

// 8. DOM 原语
const shell = createCardShell({
  title: 'Test Card',
  icon: 'target',
  count: '1/3',
  actions: [{ icon: 'plus', label: 'Add', onClick: () => {} }],
});
assert.equal(shell.root.className, 'space-card');
assert.equal(shell.root.children.length, 3, 'shell must render head/body/footer');
assert.equal(shell.head.textContent.includes('Test Card'), true);
assert.equal(shell.head.querySelectorAll('.space-card-action').length, 1);
shell.setCount(0);
assert.equal(shell.head.querySelector('.space-card-count').hidden, true);
shell.setFooter('hint');
assert.equal(shell.foot.hidden, false);
assert.equal(shell.foot.textContent, 'hint');

const form = createInlineForm({ placeholder: 'x', onSubmit: () => true });
assert.equal(form.form.children.length, 2);
assert.equal(form.input.placeholder, 'x');

const row = createListRow({ label: 'task', checked: true, checkable: false });
assert.equal(row.classList.contains('is-done'), true);
assert.equal(row.classList.contains('is-static'), true);

// 9. 边界守卫：kit 不能被 dashboard/inspector 反向依赖
const kitDir = join(here, 'space-kit');
for (const file of readdirSync(kitDir).filter((name) => name.endsWith('.js'))) {
  const source = readFileSync(join(kitDir, file), 'utf8');
  assert.doesNotMatch(source, /from\s+['"][^'"]*(space-dashboard|space-inspector|space-catalog|space\.js)/, `${file} must not import dashboard/inspector`);
  assert.doesNotMatch(source, /nativeCall\(/, `${file} must not call the Native Host directly`);
}

for (const key of NEW_KEYS) {
  const slug = key.replace('widget/', '').replace(/[A-Z]/g, (char) => `-${char.toLowerCase()}`);
  const file = join(here, 'plugins', 'widgets', `${slug}.js`);
  const source = readFileSync(file, 'utf8');
  assert.match(source, /space-kit\/index\.js/, `${file} must consume the shared kit`);
  assert.doesNotMatch(source, /space-dashboard|space-inspector|space-catalog/, `${file} must stay independent from the dashboard shell`);
}

console.log('✓ space-kit metadata, helpers and boundaries verified');
