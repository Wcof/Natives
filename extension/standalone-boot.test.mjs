import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('./platform/standalone-boot.js', import.meta.url), 'utf8');

function run({ search, framed = false, classes = new Set() } = {}) {
  const documentElement = {
    classList: {
      add: (name) => classes.add(name),
      contains: (name) => classes.has(name),
    },
  };
  const self = {};
  vm.runInNewContext(source, {
    location: { search },
    document: { documentElement },
    URLSearchParams,
    window: framed ? { top: {}, self } : { top: self, self },
  });
  return classes;
}

// ?standalone=1 进入独立运行；其余参数、缺省与非 1 值不受影响（ADR-0048 D5.2）。
assert.deepEqual(run({ search: '?standalone=1' }), new Set(['standalone']));
assert.deepEqual(run({ search: '?app=fund&standalone=1&theme=volt' }), new Set(['standalone']));
assert.deepEqual(run({ search: '?app=fund' }), new Set());
assert.deepEqual(run({ search: '?standalone=0' }), new Set());
assert.deepEqual(run({ search: '?standalone=true' }), new Set());
assert.deepEqual(run({ search: '' }), new Set());

// 被宿主嵌入时同样进入独立运行：旧 Host 未带参数、参数被剥离也不会退回带侧栏的形态。
assert.deepEqual(run({ search: '', framed: true }), new Set(['standalone']));
assert.deepEqual(run({ search: '?theme=volt', framed: true }), new Set(['standalone']));

// 四个宿主页面必须在 head 引入该脚本，保证类在首帧前生效。
for (const page of ['app.html', 'space.html', 'files.html', 'apps.html']) {
  const html = readFileSync(new URL(`./${page}`, import.meta.url), 'utf8');
  const head = html.slice(0, html.indexOf('</head>'));
  assert.match(head, /<script src="platform\/standalone-boot\.js"><\/script>/, `${page} head`);
}

// 嵌入布局契约：侧栏整列隐藏，并把仍写死 grid-column:2 的行拉回整行，
// 避免单列模板留下一条空轨道（看起来仍是多出来的一条左栏）。
const required = {
  'space.css': ['html.standalone #app-sidebar'],
  'files.css': [
    'html.standalone #app-sidebar',
    'html.standalone .toolbar',
    'html.standalone #status-bar',
    'grid-column:1 / -1',
  ],
  'app.css': ['html.standalone .app-shell-nav'],
  'apps.css': ['html.standalone .apps-nav'],
};
for (const [file, needles] of Object.entries(required)) {
  const css = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8').replace(/\s+/g, ' ');
  for (const needle of needles) {
    assert.ok(css.includes(needle), `${file} 缺少嵌入布局规则：${needle}`);
  }
}

// 嵌入时不再保留「开合自身左侧栏」的控件：外壳页面的按钮由 CSS 隐藏，
// 投资模块头部的折叠按钮（它折叠的是外壳侧栏）也隐藏并停止联动。
const repoRoot = new URL('..', import.meta.url);
const readRepo = (relative) => readFileSync(new URL(relative, repoRoot), 'utf8');
assert.ok(
  readRepo('modules/fund/ui/src/styles/layout.css').includes('html.standalone #btn-toggle-left-sidebar'),
  '投资模块缺少嵌入时隐藏左栏按钮的规则'
);
assert.ok(
  readRepo('modules/fund/ui/src/app.js').includes('embeddedShell'),
  '投资模块应按嵌入态跳过左栏按钮联动'
);
assert.ok(
  readRepo('extension/apps.css').includes('html.standalone #apps-expand-sidebar-btn'),
  '应用中心缺少嵌入时隐藏展开按钮的规则'
);
// 模块页永远在 iframe 里：只能用 ?standalone=1 判定，不能用 framed，
// 否则 Chrome 扩展里的模块也会被当成宿主嵌入。
for (const file of ['modules/fund/ui/src/partials/head.html', 'modules/tokenusage/ui/src/index.html']) {
  const head = readRepo(file);
  assert.match(head, /get\(["']standalone["']\)/, `${file} 应读取 standalone 参数`);
  assert.equal(head.includes('window.top !== window.self'), false, `${file} 不得用 framed 判定`);
}

// 解析环境缺失（无 location/window）时静默跳过，不得阻塞页面加载。
assert.doesNotThrow(() => vm.runInNewContext(source, {}));

console.log('standalone-boot.test.mjs: ok');
