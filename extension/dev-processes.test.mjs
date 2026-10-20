// 构建前清理使用真实 dev 函数、隔离进程表；不终止用户进程。
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createContext,runInContext} from 'node:vm';
const source=readFileSync(new URL('./dev.mjs',import.meta.url),'utf8');
const functions=source.slice(source.indexOf('function posixHostPids('),source.indexOf('function extensionId('));
const paths=['/repo/target/debug/model-host','/repo/target/debug/natives-statusbar','/repo/target/release/native-file-host'];
const stamp='Tue Oct  6 20:09:14 2026';
function fixture({stubborn=false,reused=false,changedPath=false,respawn=false}={}){
  const rows=new Map([[10,paths[0]+' --model-host-worker'],[11,paths[0]+' chrome-extension://test/'],[12,paths[1]],[13,paths[2]],[14,'/other/target/debug/model-host'],[15,'/Library/Application Support/Natives/hosts/model-host']]);
  const signals=[];
  const context=createContext({projectHostPaths:paths,platform:()=> 'darwin',setTimeout,console:{log(){}},
    process:{pid:999,kill(pid,signal){signals.push([pid,signal]);if(respawn&&pid===11)rows.set(20,paths[0]+' new-client');if(!stubborn||signal==='SIGKILL'||pid!==10)rows.delete(pid);}},
    spawnSync(_cmd,args){
      if(args[0]==='-p'){
        const pid=Number(args[1]);if(!rows.has(pid))return {status:1,stdout:''};
        return {status:0,stdout:(reused&&pid===10?'Tue Oct  6 20:10:14 2026':stamp)+' '+(changedPath&&pid===10?'/other/target/debug/model-host':rows.get(pid))};
      }
      return {status:0,stdout:[...rows].map(([pid,cmd])=>pid+' '+stamp+' '+cmd).join('\n')};
    }});
  runInContext(functions+';globalThis.stop=stopProjectHosts;',context);
  return {rows,signals,stop:context.stop};
}
const normal=fixture();assert.equal(await normal.stop(),4);
assert.deepEqual(normal.signals.map(([pid])=>pid),[11,12,13,10],'clients and launcher stop before shared worker');
assert.deepEqual([...normal.rows.keys()],[14,15],'other repo and installed product survive');
const stubborn=fixture({stubborn:true});await stubborn.stop();assert.ok(stubborn.signals.some(([pid,signal])=>pid===10&&signal==='SIGKILL'));
for(const options of [{reused:true},{changedPath:true}]){
  const f=fixture(options);await assert.rejects(f.stop(),/待替换/);assert.ok(!f.signals.some(([pid])=>pid===10),'reused or different executable PID must not be signalled');
}
const restarted=fixture({respawn:true});await assert.rejects(restarted.stop(),/待替换/);assert.ok(!restarted.signals.some(([pid])=>pid===20),'new occupancy blocks build');
assert.ok(source.indexOf('await stopProjectHosts();')<source.indexOf("run('rtk', ['env'"),'cleanup precedes first build');
console.log('dev restart: old clients/worker/release cleanup, foreign/install exclusion, TERM/KILL, PID identity and new occupancy guards passed');
