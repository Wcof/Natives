import assert from 'node:assert/strict';
import { resolveTheme, themeParam } from './platform/theme-param.js';

// 合法值覆盖；缺省与非法值回退已保存偏好（ADR-0048 D5）。
assert.equal(themeParam('?theme=volt'), 'volt');
assert.equal(themeParam('?theme=archive'), 'archive');
assert.equal(themeParam('?app=fund&theme=volt'), 'volt');
assert.equal(themeParam('?theme=VOLT'), null);
assert.equal(themeParam('?theme='), null);
assert.equal(themeParam('?theme=dark'), null);
assert.equal(themeParam(''), null);
assert.equal(themeParam(undefined), null);

assert.equal(resolveTheme('archive', '?theme=volt'), 'volt');
assert.equal(resolveTheme('volt', '?theme=archive'), 'archive');
// 缺省：保留已保存偏好
assert.equal(resolveTheme('volt', ''), 'volt');
assert.equal(resolveTheme('archive', '?app=tokenusage'), 'archive');
// 非法值与未知保存值都回到 archive，不写入任何偏好
assert.equal(resolveTheme('volt', '?theme=nope'), 'volt');
assert.equal(resolveTheme('sepia', '?theme=nope'), 'archive');
assert.equal(resolveTheme(undefined, undefined), 'archive');

// 解析不得改写输入（页面只读 URL，不回写存储）。
const search = '?theme=volt';
themeParam(search);
assert.equal(search, '?theme=volt');

console.log('theme-param.test.mjs: ok');
