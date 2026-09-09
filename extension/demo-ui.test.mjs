import assert from 'node:assert/strict';
import { readAllResource } from './apps/demo-ui.js';

const source = new TextEncoder().encode(JSON.stringify({ text: `中文${'x'.repeat(1024 * 1024)}` }));
const calls = [];
const client = { call: async (_method, params) => {
  calls.push(params);
  const bytes = source.subarray(params.offset, params.offset + params.length);
  return {
    app_id: params.appId, package_id: params.packageId, version: '2.0.0', format: 'json',
    total_size: source.length, offset: params.offset, length: bytes.length,
    data: Buffer.from(bytes).toString('base64'),
  };
} };
const result = await readAllResource(client, 'com.natives.app.demo', 'demo-data');
assert.deepEqual(result.bytes, source);
assert.equal(JSON.parse(new TextDecoder().decode(result.bytes)).text.startsWith('中文'), true);
assert.equal(calls.length, 3, 'a resource over 1 MiB must be read in 512 KiB chunks');

let count = 0;
await assert.rejects(() => readAllResource({ call: async (_method, params) => ({
  app_id: params.appId, package_id: params.packageId, version: count++ ? '2.1.0' : '2.0.0', format: 'json',
  total_size: 600_000, offset: params.offset, length: Math.min(params.length, 600_000 - params.offset),
  data: Buffer.alloc(Math.min(params.length, 600_000 - params.offset)).toString('base64'),
}) }, 'com.natives.app.demo', 'demo-data'), /resource/);
console.log('demo resources: UTF-8, chunking and version consistency passed');
