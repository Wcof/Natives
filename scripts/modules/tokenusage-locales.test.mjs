import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {test} from 'node:test';
const source=readFileSync(new URL('../../modules/tokenusage/ui/src/locale.js',import.meta.url),'utf8');
const html=readFileSync(new URL('../../modules/tokenusage/ui/src/index.html',import.meta.url),'utf8');
test('Tokenusage uses the same defined keys in both languages and all HTML roles',()=>{
 const context={window:{},document:{documentElement:{},querySelectorAll:()=>[]}};
 vm.runInNewContext(source,context);
 const {t,apply}=context.window.TokenLocale;
 const keys=[...html.matchAll(/data-i18n(?:-title|-aria)?="([^"]+)"/g)].map(match=>match[1]);
 for(const language of ['zh_CN','en']){
  apply(language);
  for(const key of keys)assert.notEqual(t(key),key,`${language}: ${key}`);
  assert.notEqual(t('unavailable'),'unavailable');
 }
 assert.ok(!/[🏠🎯🛠💬🤖📈⚡]/u.test(html),'product controls use text and controlled SVG');
});
