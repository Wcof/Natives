import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createPreviewControllers} from './files-preview-controllers.js';

class Element {
  children = []; dataset = {}; attributes = {}; classList = {toggle() {}};
  append(...nodes) { for (const node of nodes) { node.parent = this; this.children.push(node); } }
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter(node => node !== this); }
  setAttribute(key, value) { this.attributes[key] = value; }
  querySelector(selector) {
    return this.children.find(node => selector.startsWith('.') ? node.className === selector.slice(1) : node.dataset.action === 'openEditor') || null;
  }
}
const body = new Element(), layout = new Element(), preview = new Element();
globalThis.document = {createElement: () => new Element(), querySelector: () => layout};
Object.defineProperty(globalThis, 'navigator', {value: {pdfViewerEnabled: true}, configurable: true});
const originalCreate = URL.createObjectURL, originalRevoke = URL.revokeObjectURL;
const blobs = [], revoked = [], calls = [];
URL.createObjectURL = blob => { blobs.push(blob); return `blob:isolated/${blobs.length}`; };
URL.revokeObjectURL = url => revoked.push(url);
let item = {path: '/isolated/file.pdf', name: 'file.pdf', kind: 'pdf'}, response;
const valid = {mimeType: 'application/pdf', data: btoa('%PDF-1.7\ncompressed objects fixture')};
const session = {previewGeneration: 0, selectedPaths: new Set()};
const controller = createPreviewControllers({
  $: id => id === 'preview-body' ? body : id === 'preview' ? preview : null,
  call: async (method, params) => { calls.push({method, params}); return typeof response === 'function' ? response(method) : response; },
  t: (key, fallback) => fallback, session, formatSize: String,
  selectedItems: () => item ? [item] : [], isHostConnected: () => true, renderSelection() {},
});
try {
  response = valid; await controller.renderPreviewSelection();
  const frame = body.querySelector('.pdf-preview');
  assert.ok(frame); assert.equal(frame.attributes.sandbox, undefined);
  assert.match(frame.src, /^blob:isolated\/1#page=1&view=FitH&toolbar=1$/);
  assert.equal(blobs[0].type, 'application/pdf');
  controller.resetPreviewNow(); assert.deepEqual(revoked, ['blob:isolated/1']);

  for (const invalid of [{...valid, mimeType: 'text/html'}, {...valid, data: btoa('<html>bad PDF</html>')}]) {
    response = invalid; await controller.renderPreviewSelection();
    assert.equal(body.querySelector('.pdf-preview'), null);
    assert.ok(body.querySelector('.error')); assert.equal(blobs.length, 1);
  }
  const count = calls.length; navigator.pdfViewerEnabled = false;
  await controller.renderPreviewSelection();
  assert.ok(body.querySelector('.error')); assert.equal(calls.length, count, 'disabled reader must not fetch PDF bytes');
  navigator.pdfViewerEnabled = true;
  let resolve; response = method => method === 'pdf_preview' ? new Promise(done => { resolve = done; }) : undefined;
  const pending = controller.renderPreviewSelection(); controller.resetPreviewNow(); resolve(valid); await pending;
  assert.equal(body.querySelector('.pdf-preview'), null); assert.equal(blobs.length, 1);
  assert.ok(calls.some(call => call.method === 'preview_cancel'));
  response = valid; await controller.renderPreviewSelection();
  body.querySelector('.pdf-preview').onerror(); assert.ok(body.querySelector('.error'));
  controller.resetPreviewNow(); assert.equal(revoked.length, 2);
  const manifest = JSON.parse(await readFile(new URL('./manifest.json', import.meta.url)));
  assert.equal(manifest.content_security_policy.extension_pages, "script-src 'self'; object-src 'none'; frame-src http://127.0.0.1:* blob:");
} finally { URL.createObjectURL = originalCreate; URL.revokeObjectURL = originalRevoke; }
console.log('PDF fixed MIME/magic, native reader fallback, cancellation and Object URL ownership passed');
