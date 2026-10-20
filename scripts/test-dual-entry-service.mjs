// Dual-entry service mode & Native relay integration test (ADR-0042 / T02)
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import {mkdirSync,mkdtempSync} from 'node:fs';
import {resolve,join} from 'node:path';
import http from 'node:http';
import net from 'node:net';
import {randomBytes} from 'node:crypto';

const PORT = await new Promise(done=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>done(port));});});
const BOOTSTRAP = randomBytes(32).toString('base64url');

function get(path, headers = {}) {
  return new Promise((resolve, reject) => {
    http.get(`http://127.0.0.1:${PORT}${path}`, { headers }, (res) => {
      let data = '';
      res.on('data', (c) => (data += c));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, data }));
    }).on('error', reject);
  });
}

function post(path, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const postData = JSON.stringify(body);
    const req = http.request(
      `http://127.0.0.1:${PORT}${path}`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(postData),
          ...headers,
        },
      },
      (res) => {
        let data = '';
        res.on('data', (c) => (data += c));
        res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, data }));
      }
    );
    req.on('error', reject);
    req.write(postData);
    req.end();
  });
}

function sendNativeFrame(child, reqObj) {
  return new Promise((resolve, reject) => {
    let buffer=Buffer.alloc(0);
    const finish=(error,value)=>{clearTimeout(timer);child.stdout.off('data',onData);child.off('exit',onExit);error?reject(error):resolve(value);};
    const onExit=()=>finish(Error('Native relay exited before response'));
    const onData=data=>{
      buffer=Buffer.concat([buffer,data]);
      while(buffer.length>=4){
        const length=buffer.readUInt32LE(0);
        if(length>1024*1024){finish(Error('oversized Native response'));return;}
        if(buffer.length<length+4)return;
        const body=JSON.parse(buffer.subarray(4,length+4).toString());buffer=buffer.subarray(length+4);
        if(body.id===reqObj.id){finish(null,body);return;}
      }
    };
    const timer=setTimeout(()=>finish(Error('Native relay timeout')),5000);
    child.stdout.on('data',onData);child.once('exit',onExit);
    const payload=Buffer.from(JSON.stringify(reqObj)),header=Buffer.alloc(4);header.writeUInt32LE(payload.length);child.stdin.write(Buffer.concat([header,payload]));
  });
}

mkdirSync('.tmp/dual-entry-service',{recursive:true});
const isolated = mkdtempSync(resolve('.tmp/dual-entry-service/run-'));
const modelPort = await new Promise(done=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>done(port));});});
const env = {...process.env,NATIVES_MODEL_HOST_AIQUERY_PORT:String(modelPort),NATIVES_MODEL_HOST_AIQUERY_URL:`http://127.0.0.1:${modelPort}`,HOME:join(isolated,'home'),NATIVES_WEB_PORT:String(PORT),NATIVES_WORKSPACE_DB:join(isolated,'workspace.db'),NATIVES_APPS_ROOT:join(isolated,'apps'),NATIVES_STATE_DIR:join(isolated,'state'),NATIVES_MODEL_HOST_CONFIG_DIR:join(isolated,'model')};
mkdirSync(env.HOME,{recursive:true});
console.log('--- Starting Dual-Entry Service Integration Test ---');

const children = [];
function ownedSpawn(...args) { const child=spawn(...args);children.push(child);return child; }
const srv = ownedSpawn('./target/debug/native-file-host', [
  '--service',
  '--port',
  String(PORT),
  '--bootstrap',
  BOOTSTRAP,
],{env});

srv.stderr.on('data', (d) => {
  const line = d.toString().trim();
  if (line) console.log('[Service log]', line);
});

// Wait for service ready
await new Promise((r) => setTimeout(r, 800));

try {
  // 1. Health check
  const health = await get('/healthz');
  assert.equal(health.status, 200, 'healthz must return 200');
  const healthData = JSON.parse(health.data);
  assert.equal(healthData.ok, true);
  assert.equal(healthData.service, 'native-file-host');
  assert.equal(healthData.port, PORT);
  console.log('✓ Healthz check passed');

  // 2. F01 P0 check: Unauthenticated relay request with untrusted Origin must be rejected
  const fakeRelay = await post(
    '/native/relay',
    { id: '1', method: 'version', params: {} },
    {
      Origin: 'https://untrusted.invalid',
      Host: `127.0.0.1:${PORT}`,
    }
  );
  assert.equal(fakeRelay.status, 403, 'unauthenticated relay with Origin must be 403 Forbidden');
  console.log('✓ F01 P0 unauthenticated relay rejection passed');

  // 3. Bootstrap token exchange
  const exRes = await post(
    '/web/session/exchange',
    { bootstrapToken: BOOTSTRAP },
    {
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
    }
  );
  assert.equal(exRes.status, 200, 'exchange must succeed with 200');
  assert.ok(exRes.headers['set-cookie'], 'must set session cookie');
  const cookie = exRes.headers['set-cookie'][0].split(';')[0];
  const csrfToken = JSON.parse(exRes.data).csrfToken;
  assert.ok(csrfToken, 'must return csrf token');
  console.log('✓ Bootstrap exchange and cookie passed');

  // 4. Single-use consumption
  const exReplay = await post(
    '/web/session/exchange',
    { bootstrapToken: BOOTSTRAP },
    {
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
    }
  );
  assert.equal(exReplay.status, 401, 'replayed bootstrap token must fail');
  console.log('✓ Bootstrap token single-use rejection passed');

  // 5. Authenticated session query
  const sessRes = await get('/web/session', {
    Cookie: cookie,
    Host: `127.0.0.1:${PORT}`,
  });
  assert.equal(sessRes.status, 200);
  const sessData = JSON.parse(sessRes.data);
  assert.equal(sessData.authenticated, true);
  assert.equal(sessData.csrfToken, csrfToken);
  console.log('✓ Authenticated session validation passed');

  // 6. F02 check: Surface mutation without CSRF must be rejected
  const noCsrfSurf = await post(
    '/web/surfaces',
    { appId: 'fund' },
    {
      Cookie: cookie,
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
    }
  );
  assert.equal(noCsrfSurf.status, 403, 'surface registration without CSRF must be 403 Forbidden');
  console.log('✓ F02 surface mutation without CSRF rejection passed');

  // 7. F02 check: Unknown module surface must be rejected
  const unknownAppSurf = await post(
    '/web/surfaces',
    { appId: 'not-a-registered-module' },
    {
      Cookie: cookie,
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
      'X-CSRF-Token': csrfToken,
    }
  );
  assert.equal(unknownAppSurf.status, 400, 'unknown appId must be 400 Bad Request');
  console.log('✓ F02 unknown module surface rejection passed');

  // 8. Legitimate Surface registration and lease
  const surfRes = await post(
    '/web/surfaces',
    { appId: 'fund' },
    {
      Cookie: cookie,
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
      'X-CSRF-Token': csrfToken,
    }
  );
  assert.equal(surfRes.status, 200);
  const surfaceId = JSON.parse(surfRes.data).surfaceId;
  assert.ok(surfaceId.startsWith('surf_'), 'surface ID format');
  console.log('✓ Surface registration passed:', surfaceId);

  // 9. F05 check: Authenticated SSE /web/events endpoint
  const sseReq = http.request(`http://127.0.0.1:${PORT}/web/events`, {
    headers: {
      Cookie: cookie,
      Host: `127.0.0.1:${PORT}`,
    },
  });
  const sseResponsePromise = new Promise((resolve) => {
    sseReq.on('response', (res) => {
      resolve({ status: res.statusCode, contentType: res.headers['content-type'] });
      res.destroy();
    });
  });
  sseReq.end();
  const sseResult = await sseResponsePromise;
  assert.equal(sseResult.status, 200, 'SSE status must be 200');
  assert.ok(sseResult.contentType.includes('text/event-stream'), 'SSE content type must be event-stream');
  console.log('✓ F05 Authenticated SSE /web/events passed');

  // 10. F03 check: Web Model query to model-host via /web/rpc
  const modelQueryRes = await post(
    '/web/rpc',
    { id: 'mq1', method: 'model_snapshot', params: {} },
    {
      Cookie: cookie,
      Origin: `http://127.0.0.1:${PORT}`,
      Host: `127.0.0.1:${PORT}`,
      'X-CSRF-Token': csrfToken,
      'X-Natives-Surface-Id': surfaceId,
    }
  );
  assert.equal(modelQueryRes.status, 200, 'model_snapshot via Web RPC must be 200');
  const modelData = JSON.parse(modelQueryRes.data);
  assert.equal(modelData.ok, true, 'model_snapshot must return ok:true');
  assert.ok(Array.isArray(modelData.result.providers), 'model_snapshot must contain providers array');
  console.log('Model snapshot via authenticated Surface passed; live inference is outside this smoke.');

  // 11. Multiple Native Relays to single service instance with secret
  const relay1 = ownedSpawn('./target/debug/native-file-host', ['--relay', '--port', String(PORT)],{env});
  const relay2 = ownedSpawn('./target/debug/native-file-host', ['--relay', '--port', String(PORT)],{env});

  const r1Resp = await sendNativeFrame(relay1, { id: 'r1', method: 'version', params: {} });
  assert.equal(r1Resp.id, 'r1');
  assert.equal(r1Resp.ok, true);
  assert.equal(r1Resp.result.protocolVersion, 1);

  const r2Resp = await sendNativeFrame(relay2, { id: 'r2', method: 'version', params: {} });
  assert.equal(r2Resp.id, 'r2');
  assert.equal(r2Resp.ok, true);
  assert.equal(r2Resp.result.protocolVersion, 1);
  console.log('✓ Multiple Native Relays talking to single service passed');

  relay1.kill();
  const r2RespAgain = await sendNativeFrame(relay2, { id: 'r2-2', method: 'version', params: {} });
  assert.equal(r2RespAgain.ok, true);
  relay2.kill();
  console.log('✓ Closing one relay leaves other relays operating normally');

  // 12. Static file serving (e.g. files.html)
  const staticFile = await get('/files.html');
  assert.equal(staticFile.status, 200);
  assert.equal(staticFile.headers['content-type'], 'text/html; charset=utf-8');
  assert.ok(staticFile.data.includes('Natives Files'), 'contains HTML content');
  console.log('✓ Static file serving passed');

  console.log('\n--- SERVICE API/RELAY SMOKE PASSED; NOT FULL T02 OR PRODUCT ACCEPTANCE ---');
} finally {
  for(const child of children){child.stdin?.end();child.kill();}
}
