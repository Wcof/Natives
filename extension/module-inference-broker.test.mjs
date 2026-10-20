import assert from 'node:assert/strict';
import { createModuleInferenceBroker } from './module-inference-broker.js';

const replies = [];
const frame = { contentWindow: { postMessage: message => replies.push(message) } };
const context = { frame, generation: 'generation-1', challenge: 'challenge-1', capabilities: ['ai.inference.request', 'system.link.open'] };
const calls = [];
let finishStart;
const api = {
  inferenceOptions: async () => ({ models: [] }),
  inferenceStart: () => new Promise(resolve => { finishStart = resolve; }),
  inferenceCancel: async value => { calls.push(['cancel', value]); },
  inferenceStatus: async () => ({ done: true, text: 'synthetic' }),
};
let releases = 0;
const broker = createModuleInferenceBroker({ context: () => context, acquire: () => api, release: () => { releases++; }, openLink: async url => calls.push(['link', url]), openSettings: async () => {} });
const request = (type, op, extra = {}) => ({ source: frame.contentWindow, data: { type, op, requestId: crypto.randomUUID(), generation: context.generation, challenge: context.challenge, ...extra } });

await broker.handle(request('natives:ai', 'options', { challenge: 'wrong' }));
assert.equal(replies.length, 0, 'wrong challenge must receive nothing');
await broker.handle(request('natives:open-link', 'open', { url: 'javascript:alert(1)' }));
assert.match(replies.at(-1).error, /链接无效/);
assert.equal(calls.length, 0);
await broker.handle(request('natives:ai', 'start', { providerId: 'p', modelId: 'm', text: ' ' }));
assert.match(replies.at(-1).error, /输入无效/);
const pending = broker.handle(request('natives:ai', 'start', { providerId: 'p', modelId: 'm', text: 'synthetic evidence' }));
await Promise.resolve();
assert.equal(typeof finishStart, 'function');
await broker.dispose();
finishStart({ id: 'late-job' });
await pending;
assert.equal(releases, 1);
assert.deepEqual(calls, [['cancel', { owner: calls[0][1].owner, id: 'late-job' }]], 'late start must be cancelled after disposal');
assert.equal(replies.length, 2, 'stale start must not reply to disposed frame');
const broker2 = createModuleInferenceBroker({ context: () => context, acquire: () => api, release: () => {}, openSettings: async () => {} });
const starting = broker2.handle(request('natives:ai', 'start', { providerId: 'p', modelId: 'm', text: 'synthetic evidence' }));
await Promise.resolve();
await broker2.handle(request('natives:ai', 'cancel'));
assert.equal(replies.at(-1).result.cancelled, true);
finishStart({ id: 'cancelled-start' });
await starting;
assert.equal(calls.at(-1)[1].id, 'cancelled-start', 'cancel during startup must cancel the late job');
await broker2.dispose();

// 方案11 §6 回归：selection=default 走 expectedConfigToken 校验并转发；
// start 返回实际采用的 providerId/modelId/route。
const started3 = [];
const api3 = {
  inferenceOptions: async () => ({ models: [], defaultModel: { providerId: 'p', modelId: 'm', name: 'n', configToken: 'tok' } }),
  inferenceStart: async value => { started3.push(value); return { id: 'job-def', providerId: value.providerId ?? 'p', modelId: value.modelId ?? 'm', route: `route-${value.providerId ?? 'p'}` }; },
  inferenceCancel: async value => { calls.push(['cancel', value]); },
  inferenceStatus: async () => ({ done: true, text: 'ok' }),
};
const broker3 = createModuleInferenceBroker({ context: () => context, acquire: () => api3, release: () => {}, openLink: async () => {}, openSettings: async () => {} });
// 默认未带 token → 拒绝。
await broker3.handle(request('natives:ai', 'start', { selection: 'default', text: 'synthetic evidence' }));
assert.match(replies.at(-1).error, /推理输入无效/);
// 默认带 token → 转发 selection/expectedConfigToken，返回真实身份。
await broker3.handle(request('natives:ai', 'start', { selection: 'default', expectedConfigToken: 'tok', text: 'synthetic evidence' }));
assert.equal(replies.at(-1).error, undefined);
assert.equal(replies.at(-1).result.started, true);
assert.equal(replies.at(-1).result.providerId, 'p');
assert.equal(replies.at(-1).result.modelId, 'm');
assert.equal(replies.at(-1).result.route, 'route-p');
assert.deepEqual(started3.at(-1), { owner: started3.at(-1).owner, text: 'synthetic evidence', selection: 'default', expectedConfigToken: 'tok' });
// 显式 pair 仍兼容且不被注入 selection 字段。
await broker3.dispose();
const broker4 = createModuleInferenceBroker({ context: () => context, acquire: () => api3, release: () => {}, openLink: async () => {}, openSettings: async () => {} });
await broker4.handle(request('natives:ai', 'start', { providerId: 'p2', modelId: 'm2', text: 'synthetic evidence' }));
assert.equal(replies.at(-1).result.providerId, 'p2');
assert.deepEqual(started3.at(-1), { owner: started3.at(-1).owner, text: 'synthetic evidence', providerId: 'p2', modelId: 'm2' });
await broker4.dispose();
console.log('inference broker security and disposal passed');

const broker5 = createModuleInferenceBroker({ context: () => context, acquire: () => api3, release: () => {}, openSettings: async () => {} });
for(const maxTokens of [0,511,8193,4096.5,'4096',null]) {
 const before=started3.length;
 await broker5.handle(request('natives:ai','start',{providerId:'p',modelId:'m',text:'synthetic evidence',maxTokens}));
 assert.match(replies.at(-1).error,/输入无效/);assert.equal(started3.length,before);
}
await broker5.handle(request('natives:ai','start',{selection:'default',expectedConfigToken:'tok',text:'synthetic evidence',maxTokens:4096}));
assert.equal(started3.at(-1).maxTokens,4096);
await broker5.dispose();
