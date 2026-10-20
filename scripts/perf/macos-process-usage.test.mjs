import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {macosProcessUsage} from './macos-process-usage.mjs';
if(process.platform==='darwin') {
  const parent=spawn('python3',['-u','-c',`import subprocess,sys,time
print('ready',flush=True)
sys.stdin.readline()
subprocess.run([sys.executable,'-c','import time; end=time.process_time()+0.6; exec("while time.process_time()<end: pass")'],check=True)
print('reaped',flush=True)
sys.stdin.readline()
`],{stdio:['pipe','pipe','inherit']});
  const lines=createInterface({input:parent.stdout})[Symbol.asyncIterator]();
  try {
    assert.equal((await lines.next()).value,'ready');
    const before=macosProcessUsage([parent.pid]).get(parent.pid);
    parent.stdin.write('\n');assert.equal((await lines.next()).value,'reaped');
    const after=macosProcessUsage([parent.pid]).get(parent.pid);
    assert.equal(after.started,before.started);
    assert.ok(after.reapedChildCpuMs-before.reapedChildCpuMs>=500,JSON.stringify({before,after}));
    assert.ok(after.ownCpuMs>=before.ownCpuMs);
    assert.ok(after.residentKiB>0);
    console.log(JSON.stringify({status:'PASS',calibration:'reaped child burns 600 ms CPU',childCpuMs:after.reapedChildCpuMs-before.reapedChildCpuMs}));
  } finally {parent.stdin.end();parent.kill();}
} else console.log('macOS calibration not applicable on this OS');
