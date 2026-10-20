import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {runInNewContext} from 'node:vm';
import {fundLocalesSource,isFundModuleKey} from './fund-locales.mjs';
const root=resolve(import.meta.dirname,'../..');
test('Every Fund HTML locale reference belongs to the module and reaches both embedded catalogs',()=>{
 const tables={};runInNewContext(fundLocalesSource(root),{Fund:{registerMessages:(language,messages)=>{tables[language]=messages;}}});
 const scan=directory=>{for(const entry of readdirSync(directory,{withFileTypes:true})){
  const path=join(directory,entry.name);if(entry.isDirectory()){scan(path);continue;}if(!path.endsWith('.html'))continue;
  for(const [,key] of readFileSync(path,'utf8').matchAll(/data-i18n[^=\s]*=["'](fund\w+)["']/g)){
   assert.ok(isFundModuleKey(key),`${key} in ${entry.name} has no Fund module locale owner`);
   for(const language of ['zh-CN','en'])assert.ok(tables[language][key],`${key} is missing from embedded ${language}`);
  }
 }};
 scan(join(root,'modules/fund/ui/src'));
});
