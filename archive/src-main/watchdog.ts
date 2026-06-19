// ── Watchdog ──
// PID polling: probe parent process every 2s.
// If parent dies, this child process exits itself.
// Injected via: node --require dist/watchdog.js

const POLL_INTERVAL = 2000; // 2 seconds
const parentPid = process.ppid;

function poll(): void {
  try {
    // process.kill(pid, 0) — signal 0 tests whether the process exists
    process.kill(parentPid, 0);
  } catch {
    // Parent process is dead — exit this child
    process.exit(1);
  }
}

// Start polling
setInterval(poll, POLL_INTERVAL);

// Also poll immediately on start
poll();
