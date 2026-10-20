import {spawnSync} from 'node:child_process';

// proc_pid_rusage includes reaped child CPU, which periodic ps cannot retain.
export function macosProcessUsage(pids) {
  if (process.platform !== 'darwin') throw Error('This resource sampler requires macOS libproc');
  const result = spawnSync('python3', ['-c', `
import ctypes,json,sys
class Usage(ctypes.Structure):
    _fields_=[('uuid',ctypes.c_ubyte*16)]+[(name,ctypes.c_uint64) for name in ['user','system','wakeups','interrupts','pageins','wired','resident','footprint','started','exited','child_user','child_system','child_wakeups','child_interrupts','child_pageins','child_elapsed']]
lib=ctypes.CDLL('/usr/lib/libproc.dylib',use_errno=True)
lib.proc_pid_rusage.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_void_p]
class Timebase(ctypes.Structure):
    _fields_=[('numer',ctypes.c_uint32),('denom',ctypes.c_uint32)]
timebase=Timebase()
ctypes.CDLL('/usr/lib/libSystem.B.dylib').mach_timebase_info(ctypes.byref(timebase))
milliseconds=timebase.numer/timebase.denom/1e6
rows=[]
for pid in map(int,sys.argv[1:]):
    usage=Usage()
    if lib.proc_pid_rusage(pid,1,ctypes.byref(usage)):
        rows.append({'pid':pid,'error':ctypes.get_errno()})
    else:
        rows.append({'pid':pid,'ownCpuMs':(usage.user+usage.system)*milliseconds,'reapedChildCpuMs':(usage.child_user+usage.child_system)*milliseconds,'residentKiB':usage.resident/1024,'started':str(usage.started)})
print(json.dumps(rows))
`, ...pids.map(String)], {encoding:'utf8',timeout:3000});
  if (result.status !== 0) throw Error(`libproc sampling failed: ${result.stderr}`);
  return new Map(JSON.parse(result.stdout).map(row=>[row.pid,row]));
}
