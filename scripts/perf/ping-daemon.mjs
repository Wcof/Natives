#!/usr/bin/env node
// Minimal UDS handshake + daemon.ping for perf scripts.
// Usage: node ping-daemon.mjs <socket> <bootstrap>
import net from 'node:net';
import { randomUUID } from 'node:crypto';

const [, , socketPath, bootstrap] = process.argv;
if (!socketPath || !bootstrap) {
  console.error('usage: ping-daemon.mjs <socket> <bootstrap>');
  process.exit(2);
}

const sock = net.createConnection(socketPath);
let buffer = '';
let responded = false;

const send = (obj) => {
  sock.write(JSON.stringify(obj) + '\n');
};

const timeout = setTimeout(() => {
  if (!responded) {
    console.error('ping timeout');
    process.exit(1);
  }
}, 5000);

sock.on('connect', () => {
  send({
    client_version: '2.0.0',
    client_id: `perf-${randomUUID()}`,
    bootstrap_token: bootstrap,
  });
});

sock.on('data', (chunk) => {
  buffer += chunk.toString('utf8');
  let idx;
  while ((idx = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, idx).trim();
    buffer = buffer.slice(idx + 1);
    if (!line) continue;
    const msg = JSON.parse(line);
    if (msg.session_token) {
      // Handshake accepted; send a ping.
      send({
        protocol_version: '2.0.0',
        request_id: randomUUID(),
        client_id: `perf-${randomUUID()}`,
        session_token: msg.session_token,
        method: 'daemon.ping',
        params: {},
      });
      continue;
    }
    if (msg.request_id || msg.success !== undefined || msg.error) {
      responded = true;
      clearTimeout(timeout);
      sock.end();
      process.exit(msg.success ? 0 : 1);
    }
  }
});

sock.on('error', (err) => {
  console.error(`ping error: ${err.message}`);
  process.exit(1);
});
