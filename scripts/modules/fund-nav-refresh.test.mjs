import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source=readFileSync(new URL('../../modules/fund/ui/src/core.js',import.meta.url),'utf8');
const body=source.slice(source.indexOf('// Shared NAV sync owner:'),source.indexOf('// \u6302\u8f7d\u547d\u540d\u7a7a\u95f4'));
let now=Date.UTC(2026,9,13,11),calls=0;
class Clock extends Date {static now(){return now;}}
const context={Date:Clock,Fund:{state:{quoteRefreshSeconds:3}},api:async()=>{calls++;return {added:0,updated:0};},document:{dispatchEvent(){}},Event};
vm.createContext(context);vm.runInContext('(()=>{'+body+'Fund.syncHoldingNav=syncHoldingNav;})();',context);
const rows=[{fundCode:'110022',navDate:'2026-10-12'}],signal=new AbortController().signal;
await context.Fund.syncHoldingNav(rows,signal);assert.equal(calls,1);
now+=31000;await context.Fund.syncHoldingNav(rows,signal);
assert.equal(calls,2,'unpublished NAV must be checked again promptly instead of staying frozen for 30 minutes');
await context.Fund.syncHoldingNav([{...rows[0],navDate:'2026-10-13'}],signal);
assert.equal(calls,2,'already published NAV is not fetched again');
console.log('Fund NAV: bounded prompt polling and published-day stop passed');
