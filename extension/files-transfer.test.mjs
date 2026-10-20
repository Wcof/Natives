import assert from 'node:assert/strict';
import { createFileOperations } from './files-operations.js';

globalThis.document = { querySelector: () => null };
const notices = [], chunks = [];
let ended = false, cancelVisible = false, modal, operations;
const bytes = new Uint8Array(2 * 1024 * 1024 + 17).fill(37);
operations = createFileOperations({
  $: () => ({ hidden: true }), t: (_key, fallback) => fallback,
  session: { currentPath: '/another', selectedPaths: new Set() },
  selectedItems: () => [{path:'/isolated/one.txt'},{path:'/isolated/two.txt'}],
  setStatus: () => {}, toast: (message, kind) => notices.push({ message, kind }),
  updateProgress: () => {}, setOperationCancelable: value => { cancelVisible = value; },
  openModal: value => { modal = value; },
  call: async (method, params, requestId) => {
    assert.ok(Buffer.byteLength(JSON.stringify({ id: requestId || 'request', method, params })) <= 256 * 1024,
      'base64 import envelope must fit the unchanged Core HTTP body limit');
    if (method === 'import_chunk') chunks.push(Buffer.from(params.data, 'base64'));
    if (method === 'import_end') { ended = true; return { path: '/isolated/big.bin' }; }
    if (method === 'extract_archive') {
      assert.equal(cancelVisible, true, 'extraction must expose the existing cancel action');
      assert.equal(operations.getBatch().requestId, requestId);
      operations.cancelActive();
      return { files: 10 };
    }
    return {};
  },
});
await operations.importFileList([new File([bytes], 'big.bin')], '/isolated');
assert.equal(ended, true, 'large import must complete instead of silently reporting a failed file');
assert.deepEqual(Buffer.concat(chunks), Buffer.from(bytes));
await operations.extractArchive({ path: '/isolated/archive.zip' });
await modal.submit('/isolated/extracted');
assert.equal(operations.isBusy(), false);
assert.equal(cancelVisible, false);
assert.ok(!notices.some(entry => entry.message.includes('个文件已解压') && entry.kind === 'success'),
  'a cancelled extraction must not show success even if its completion raced cancellation');
console.log('Large file chunk envelope and actual extraction UI cancellation passed');

const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
const clipboardWrites = [];
Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { clipboard: { writeText: async text => clipboardWrites.push(text) } } });
try {
  await operations.copyPathSelectedFor('/isolated/single.txt');
  await operations.copyPathSelected();
  assert.deepEqual(clipboardWrites, ['/isolated/single.txt', '/isolated/one.txt\n/isolated/two.txt']);
} finally {
  if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
  else delete globalThis.navigator;
}
