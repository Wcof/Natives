import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

const source=readFileSync(new URL('../../modules/fund/ui/src/app.js',import.meta.url),'utf8');
const marker='session.startScheduler(function(tick) {';
const start=source.indexOf(marker),end=source.indexOf('\n  });',start);
assert.ok(start>=0&&end>start);
let manual=0,automatic=0,positions=0;
const tab={id:'view-home'},views={'view-home':{hidden:false},'view-positions':{hidden:true}};
const context={document:{visibilityState:'visible',querySelector:()=>tab,getElementById:id=>views[id]},
  actions:{isStockDetailActive:()=>false,refreshTickers:()=>{},refreshHome:()=>manual++,refreshHomeValuation:()=>automatic++,refreshPositionsQuotes:()=>positions++},
  Fund:{},watchlistPollCount:0,positionsPollCount:0,homePollCount:0};
vm.createContext(context);
vm.runInContext('globalThis.tick=function(tick){'+source.slice(start+marker.length,end)+'}',context);
context.tick({open:false,manual:false,reason:'init'});assert.equal(automatic,0);
context.tick({open:true,manual:true,reason:'manual'});assert.equal(manual,1);
for(let i=0;i<3;i++)context.tick({open:true,manual:false,reason:null});assert.equal(automatic,3);
context.tick({open:false,manual:false,reason:null});assert.equal(automatic,4);
context.document.visibilityState='hidden';context.tick({open:false,manual:false,reason:null});assert.equal(automatic,4);
context.document.visibilityState='visible';tab.id='view-positions';views['view-home'].hidden=true;views['view-positions'].hidden=false;
context.tick({open:true,manual:true,reason:'manual'});assert.equal(positions,1);assert.equal(manual,1);
context.tick({open:false,manual:false,reason:null});assert.equal(positions,2,'closed-market ticks must still check published fund NAV');
console.log('Home/Positions shared scheduler: manual, open throttle, closed, hidden and initial de-duplication passed');

let timerDelay=0,listener;
class OpenClock extends Date {getDay(){return 5;}getHours(){return 10;}getMinutes(){return 0;}}
const sessionContext={Fund:{state:{quoteRefreshSeconds:30}},Date:OpenClock,document:{addEventListener:(_,fn)=>listener=fn},setTimeout:(_,delay)=>{timerDelay=delay;return 1;},clearTimeout:()=>{}};
vm.createContext(sessionContext);vm.runInContext(readFileSync(new URL('../../modules/fund/ui/src/market-session.js',import.meta.url),'utf8'),sessionContext);
sessionContext.Fund.marketSession.startScheduler(()=>{});assert.equal(timerDelay,30000);
sessionContext.Fund.state.quoteRefreshSeconds=10;listener();assert.equal(timerDelay,10000);
sessionContext.Fund.marketSession.stopScheduler();timerDelay=0;listener();assert.equal(timerDelay,0);
console.log('Configured market refresh cadence and configuration-change rescheduling passed');
class PublicationClock extends Date {getDay(){return 2;}getHours(){return 19;}getMinutes(){return 0;}}
sessionContext.Date=PublicationClock;sessionContext.Fund.state.pendingFundNav=true;
assert.equal(sessionContext.Fund.marketSession.currentInterval(),60000,'pending NAV is checked during its post-market publication window');
sessionContext.Fund.state.pendingFundNav=false;
assert.equal(sessionContext.Fund.marketSession.currentInterval(),1800000,'settled NAV does not keep frequent closed-market polling');
