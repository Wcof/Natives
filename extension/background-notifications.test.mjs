import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

let listener, createCount = 0, permission = 'granted', fail = false;
const chrome = {
  runtime: {
    id: 'test-extension', lastError: null,
    getURL: path => `chrome-extension://test-extension/${path}`,
    onInstalled: { addListener() {} },
    onMessage: { addListener(fn) { listener = fn; } },
  },
  action: { onClicked: { addListener() {} } }, tabs: {},
  notifications: {
    async getPermissionLevel() { return permission; },
    create(_options, callback) {
      createCount++;
      chrome.runtime.lastError = fail ? { message: 'Permission denied' } : null;
      callback(fail ? '' : 'notification-id');
      chrome.runtime.lastError = null;
    },
  },
};
const source = readFileSync(new URL('./background.js', import.meta.url), 'utf8')
  .replace(/^import .*\n/, '');
vm.runInNewContext(source, { chrome, URL, console, setupScreenshotListeners() {} });
const sender = { id: chrome.runtime.id, url: chrome.runtime.getURL('app.html?app=fund') };
const notify = { type: 'natives:notification', payload: { title: 'Evidence ready', body: 'Review in Natives' } };
assert.equal((await listener(notify, sender)).delivered, true);
fail = true;
assert.equal((await listener(notify, sender)).delivered, false);
assert.equal((await listener({ type: 'natives:notification', payload: {} }, sender)).delivered, false);
for (const url of ['app.html.evil', 'space.html']) {
  assert.equal(listener(notify, { ...sender, url: chrome.runtime.getURL(url) }), undefined);
}
assert.equal(listener(notify, { ...sender, id: 'other' }), undefined);
assert.equal(createCount, 2, 'rejected requests must have no notification side effect');
permission = 'denied';
assert.equal((await listener({ type: 'natives:notification-status' }, sender)).permission, 'denied');
chrome.notifications.getPermissionLevel = async () => { throw Error('Unavailable'); };
assert.equal((await listener({ type: 'natives:notification-status' }, sender)).permission, 'unknown');
console.log('background notification receipt, permission and sender regressions passed');
