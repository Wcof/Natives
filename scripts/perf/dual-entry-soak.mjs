// ADR-0042 real-browser soak: the Space documents survive all module cycles.
import assert from 'node:assert/strict';
import {appendFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {frameSample} from './chrome-soak.mjs';
import {macosProcessUsage} from './macos-process-usage.mjs';

const cpuMs=value=>value.split(':').map(Number).reduce((total,part)=>total*60+part,0)*1000;
function processes(state,servicePid){
 const result=spawnSync('rtk',['proxy','ps','-axo','pid=,ppid=,rss=,time=,lstart=,command='],{encoding:'utf8',timeout:3000});
 assert.equal(result.status,0);
 const rows=result.stdout.split('\n').flatMap(line=>{
  const match=/^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\w{3}\s+\w{3}\s+\d+\s+[\d:]+\s+\d{4})\s+(.*)$/.exec(line);
  return match?[{pid:+match[1],parent:+match[2],rssKiB:+match[3],cpuMs:cpuMs(match[4]),started:match[5],command:match[6]}]:[];
 });
 const selected=new Set(rows.filter(row=>row.pid===servicePid||row.command.includes(`--user-data-dir=${state}/`)||row.command.includes(`${state}/`)).map(row=>row.pid));
 for(let changed=true;changed;){changed=false;for(const row of rows)if(selected.has(row.parent)&&!selected.has(row.pid)){selected.add(row.pid);changed=true;}}
 const owned=rows.filter(row=>selected.has(row.pid)),usage=macosProcessUsage(owned.map(row=>row.pid));
 return owned.map(({command,...row})=>({...row,usage:usage.get(row.pid),root:!selected.has(row.parent),identity:`${row.pid}:${row.started}`,role:row.pid===servicePid?'core':command.includes('--type=renderer')?'renderer':command.includes('natives-app-runtime')?'app-runtime':command.includes('model-host-process')?(command.includes('--model-host-worker')?'model-worker':'model-relay'):command.includes('/native-file-host')?'native-relay':'chrome'}));
}
async function rendererPid(context,page,label){
 const session=await context.newCDPSession(page);const marks=[];
 session.on('Tracing.dataCollected',event=>{for(const item of event.value)if(item.name===label)marks.push({pid:item.pid,name:item.name,ts:item.ts});});
 await session.send('Tracing.start',{categories:'blink.user_timing',transferMode:'ReportEvents'});
 await page.evaluate(label=>performance.mark(label),label);
 await delay(100);
 const done=new Promise(resolve=>session.once('Tracing.tracingComplete',resolve));
 await session.send('Tracing.end');await done;await session.detach();
 assert.ok(marks.length,`no renderer process trace for ${label}`);
 return marks[0];
}
export async function runDualEntrySoak({extension,web,e,w,servicePid,state,report,eurl,base,smoke=false}){
 const pages=[e,w];
 const contexts=[extension,web];
 const sessions=await Promise.all(pages.map((page,index)=>contexts[index].newCDPSession(page)));
 const samples=[],cycles=[],errors=[];let phase='warmup',stopped=false;
 const stamp=()=>new Date().toISOString();
 const sample=async()=>{
  const start=performance.now();
  const row={at:stamp(),phase,processes:processes(state,servicePid),pages:[]};
  for(let index=0;index<pages.length;index++){
   const page=pages[index];if(page.isClosed())continue;
   row.pages.push({page:index,url:page.url(),dom:await sessions[index].send('Memory.getDOMCounters'),frames:await Promise.all(page.frames().map(frameSample))});
  }
  row.samplingMs=performance.now()-start;samples.push(row);appendFileSync(`${report}/soak-samples.jsonl`,JSON.stringify(row)+'\n');
 };
 const baselineDocuments=await Promise.all([e,w].map(page=>page.evaluate(()=>({timeOrigin:performance.timeOrigin,url:location.href}))));
 const sampling=(async()=>{
  let next=performance.now();while(!stopped){try{await sample();}catch(error){errors.push({at:stamp(),error:error.message});}next+=9000;await delay(Math.max(0,next-performance.now()));}
 })();
 let rendererBefore,rendererAfter;
 const start=Date.now();
 try{
  for(const [name,duration]of [['warmup',smoke?10000:300000],['run',smoke?10000:1800000]]){
   phase=name;const deadline=Date.now()+duration;
   while(Date.now()<deadline){
    const index=cycles.length%2;const module=cycles.length%3===0?'tokenusage':'fund';
    // A real user opens the module in a tab, operates, and closes the tab.
    // The Space documents stay open; no test history is cleared or rewritten.
    const page=await contexts[index].newPage();
    try{
     await page.goto(index?`${base}/app.html?app=${module}`:eurl(`app.html?app=${module}`),{waitUntil:'load',timeout:30000});
     await page.locator('iframe').waitFor({timeout:30000});const frame=page.frameLocator('iframe');
     if(module==='fund'){await frame.locator('#tab-positions').waitFor({timeout:20000});await frame.locator('#tab-positions').click();await frame.locator('#tab-home').click();}
     else{await frame.locator('[data-view="models"]').click();await frame.locator('[data-view="home"]').click();}
     cycles.push({at:stamp(),phase,entry:index?'web':'extension',module});
    } finally { await page.close().catch(()=>{}); }
    // Native visibility stays real: no synthetic visibility events or disabled throttling.
    await e.bringToFront();await w.bringToFront();await delay(smoke?100:1000);
   }
   await sample();
   if(name==='warmup'){
    rendererBefore=await Promise.all([[extension,e],[web,w]].map(async([context,page],index)=>rendererPid(context,page,`natives-warm-${index}`)));
   }
   console.log(JSON.stringify({soak:name,cycles:cycles.length,samples:samples.length,elapsedMs:Date.now()-start}));
  }
  // Keep both Space tabs genuinely visible in their independent browser windows.
  await e.bringToFront();await w.bringToFront();
  // Let the specified last-owner grace expire before quiescent CPU sampling.
  phase='release';await delay(smoke?1000:61000);
  phase='idle';await sample();await delay(smoke?10000:60000);await sample();
  rendererAfter=await Promise.all([[extension,e],[web,w]].map(async([context,page],index)=>rendererPid(context,page,`natives-idle-${index}`)));
 }finally{
  stopped=true;await sampling;
  const documents=await Promise.all([e,w].map(page=>page.evaluate(()=>({timeOrigin:performance.timeOrigin,url:location.href})).catch(()=>null)));
  const warm=samples.filter(row=>row.phase==='warmup').at(-1),idle=samples.filter(row=>row.phase==='idle'),first=idle[0],last=idle.at(-1);
  const baseline=new Map(warm?.processes.map(row=>[row.identity,row])||[]);
  const growth=(rendererBefore||[]).map((trace,index)=>{
   const before=warm?.processes.find(row=>row.pid===trace.pid),after=last?.processes.find(row=>row.pid===trace.pid);
   return{entry:index?'web':'extension',pid:trace.pid,afterPid:rendererAfter?.[index]?.pid,beforeKiB:before?.rssKiB,afterKiB:after?.rssKiB,growthPercent:before&&after?(after.rssKiB/before.rssKiB-1)*100:null};
  });
  const roots=row=>row.processes.filter(p=>p.root).map(p=>p.identity).sort().join(',');
  const cpuCoverageComplete=Boolean(first&&last&&first!==last&&idle.every(row=>roots(row)===roots(first)&&row.processes.every(p=>p.usage&&!p.usage.error)));
  const cumulative=row=>row.processes.reduce((sum,p)=>sum+p.usage.ownCpuMs+p.usage.reapedChildCpuMs,0);
  const cpuDelta=cpuCoverageComplete?cumulative(last)-cumulative(first):null;
  const idleCpuPercent=cpuDelta!==null&&cpuDelta>=0?cpuDelta/(Date.parse(last.at)-Date.parse(first.at))*100:null;
  const gaps=samples.slice(1).map((row,index)=>Date.parse(row.at)-Date.parse(samples[index].at));
  const observedRoles=[...new Set(samples.flatMap(row=>row.processes.map(p=>p.role)))].sort();
  const modelWorkerObserved=samples.some(row=>row.processes.some(p=>p.role==='model-worker'));
  const rolePeaks=Object.fromEntries(observedRoles.map(role=>[role,Math.max(0,...samples.flatMap(row=>row.processes.filter(p=>p.role===role).map(p=>p.rssKiB)))]));
  const result={status:'not-accepted',smoke,cpuCoverageComplete,cpuCounter:'macOS libproc own + reaped-child CPU, calibrated mach timebase; root churn invalidates CPU result',observedRoles,modelWorkerObserved,rolePeaks,scope:'real isolated Chrome for Testing, Core and App Runtime development candidate; real Model Engine with in-memory test Secret adapter; no real provider or installation acceptance',startedAt:new Date(start).toISOString(),endedAt:stamp(),cycles,sampleCount:samples.length,samplingErrors:errors,baselineDocuments,documents,rendererBefore,rendererAfter,growth,idleCpuPercent,maxSampleGapMs:Math.max(...gaps)};
  const passed=!smoke&&errors.length===0&&modelWorkerObserved&&cycles.length>=20&&growth.length===2&&growth.every(row=>row.pid===row.afterPid&&row.growthPercent!==null&&row.growthPercent<=15)&&idleCpuPercent!==null&&idleCpuPercent<=2&&Math.max(...gaps)<=10000&&documents.every((doc,index)=>doc?.timeOrigin===baselineDocuments[index].timeOrigin);
  result.status=passed?'local-soak-passed':smoke?'smoke-only':'failed';
  writeFileSync(`${report}/soak-summary-${smoke?'smoke':'full'}.json`,JSON.stringify(result,null,2));
  for(const session of sessions)await session.detach();
  assert.ok(smoke||passed,JSON.stringify({growth,idleCpuPercent,maxSampleGapMs:result.maxSampleGapMs,samplingErrors:errors}));
 }
}
