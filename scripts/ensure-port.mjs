import { execSync } from 'node:child_process';
import net from 'node:net';

const PORT = 3000;

function isPortInUse(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once('error', (err) => {
      if (err.code === 'EADDRINUSE') {
        resolve(true);
      } else {
        resolve(false);
      }
    });
    server.once('listening', () => {
      server.close(() => resolve(false));
    });
    server.listen(port, '127.0.0.1');
  });
}

async function main() {
  const inUse = await isPortInUse(PORT);
  if (inUse) {
    console.log(`[ensure-port] 端口 ${PORT} 已被占用，正在释放...`);
    try {
      if (process.platform === 'win32') {
        execSync(`for /f "tokens=5" %a in ('netstat -aon ^| findstr :${PORT}') do taskkill /f /pid %a`, { stdio: 'inherit' });
      } else {
        const pids = execSync(`lsof -t -i:${PORT} -sTCP:LISTEN || true`, { encoding: 'utf-8' }).trim();
        if (pids) {
          const pidList = pids.split(/\s+/).join(' ');
          console.log(`[ensure-port] 结束占用 PID: ${pidList}`);
          execSync(`kill -9 ${pidList}`, { stdio: 'inherit' });
        }
      }
      console.log(`[ensure-port] 端口 ${PORT} 已成功释放。`);
    } catch (err) {
      console.warn(`[ensure-port] 释放端口 ${PORT} 失败:`, err?.message || err);
    }
  } else {
    console.log(`[ensure-port] 端口 ${PORT} 空闲。`);
  }
}

main();
