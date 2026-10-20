import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {notesWidget} from './widgets/notes.js';
import {MockElement, setupTestDomEnvironment} from '../test-dom-mock.js';

setupTestDomEnvironment();
const originalTimer = globalThis.setTimeout, originalClear = globalThis.clearTimeout;
const originalCreate = document.createElement, originalUrl = URL.createObjectURL, originalRevoke = URL.revokeObjectURL;
const timers = new Map(), blobs = [], downloads = [], revoked = [];
let timerId = 0, clipboardWrite = async () => {}, copies = [];
Object.defineProperty(globalThis, 'navigator', {configurable: true, value: {clipboard: {writeText: text => {copies.push(text); return clipboardWrite(text);}}}});
globalThis.setTimeout = callback => {timers.set(++timerId, callback); return timerId;};
globalThis.clearTimeout = id => timers.delete(id);
URL.createObjectURL = blob => {blobs.push(blob); return `blob:notes/${blobs.length}`;};
URL.revokeObjectURL = url => revoked.push(url);
document.createElement = tag => {
  const element = originalCreate(tag);
  if (tag === 'a') element.click = () => downloads.push({name: element.download, url: element.href});
  return element;
};

try {
  for (const lang of ['zh_CN', 'en']) {
    const messages = JSON.parse(await readFile(new URL(`../_locales/${lang}/messages.json`, import.meta.url)));
    const t = (key, fallback) => messages[key]?.message || fallback || key;
    const container = new MockElement(); let saved, saves = 0;
    const dispose = notesWidget.render(container, {content: ''}, {}, {t, onDataChange: data => {saved = data; saves++;}});
    const area = container.querySelector('textarea');
    const button = action => container.querySelectorAll('.notes-chip-btn').find(node => node.dataset.act === action);
    const click = action => button(action).onclick(new Event('click'));
    for (const [action, key] of [['prompt', 'notesPrompt'], ['arch', 'notesArchitecture'], ['diag', 'notesDiagnosis'], ['review', 'notesReview'], ['code', 'notesCode']]) {
      area.value = 'Existing content';
      await click(action);
      assert.equal(saved.content, 'Existing content\n\n' + messages[`${key}Template`].message);
      assert.equal(button(action).querySelector('span').textContent, messages[key].message);
    }
    const before = saves; area.value = '123'; area.oninput(); area.onblur();
    assert.equal(saves, before + 1, 'blur must not save input that was already submitted');
    assert.ok(container.querySelector('.note-stats-bar').textContent.includes(lang === 'en' ? 'characters' : '字'), 'first input into an empty note must display localized stats');
    await click('export');
    assert.equal(await blobs.at(-1).text(), '123'); assert.match(downloads.at(-1).name, /^note-\d{4}-\d{2}-\d{2}\.md$/);
    assert.equal(revoked.at(-1), downloads.at(-1).url);
    await click('copy'); assert.equal(copies.at(-1), '123'); assert.equal(timers.size, 1);
    assert.equal(button('copy').querySelector('span').textContent, messages.copied.message);
    clipboardWrite = async () => {throw new Error('denied');};
    await click('copy'); assert.equal(button('copy').querySelector('span').textContent, messages.clipboardUnavailable.message);
    area.value = ''; const count = copies.length; await click('copy'); assert.equal(copies.length, count, 'cleared text must never copy the old persisted content');
    let resolve; clipboardWrite = () => new Promise(done => {resolve = done;}); area.value = 'Pending copy';
    const pending = click('copy'); const oldLabel = button('copy').querySelector('span');
    dispose(); assert.equal(timers.size, 0); const value = oldLabel.textContent;
    resolve(); await pending; assert.equal(timers.size, 0); assert.equal(oldLabel.textContent, value, 'late clipboard completion must not update a disposed widget');
    clipboardWrite = async () => {};
  }
} finally {
  globalThis.setTimeout = originalTimer; globalThis.clearTimeout = originalClear;
  document.createElement = originalCreate; URL.createObjectURL = originalUrl; URL.revokeObjectURL = originalRevoke;
}
console.log('Notes bilingual templates/stats, download bytes, clipboard failure/empty text and feedback ownership passed');
