import assert from 'node:assert/strict';
import { attachPositionGestures } from './space-position-gestures.js';

globalThis.document = new EventTarget();
let frameId = 0;
const frames = new Map();
globalThis.requestAnimationFrame = fn => { frames.set(++frameId, fn); return frameId; };
globalThis.cancelAnimationFrame = id => frames.delete(id);
const paint = () => { for (const [id, fn] of frames) { frames.delete(id); fn(); } };
const flush = () => new Promise(resolve => setImmediate(resolve));
function fixture(commitDisplay) {
  const captured = new Set(), errors = [];
  const element = {
    style: {}, getRootNode: () => ({ host: { offsetWidth: 1000, offsetHeight: 1000 } }),
    setPointerCapture: id => captured.add(id), hasPointerCapture: id => captured.has(id),
    releasePointerCapture: id => captured.delete(id), focus() {}, removeAttribute() {},
  };
  const scaleHandle = {}, rotHandle = {};
  const dispose = attachPositionGestures({ element, scaleHandle, rotHandle,
    display: { xPercent: 40, yPercent: 40 }, commitDisplay, reportError: error => errors.push(error) });
  const pointer = (x, y = 0) => ({ button: 0, pointerId: 1, clientX: x, clientY: y, preventDefault() {}, stopPropagation() {} });
  const key = value => {
    const event = new Event('keydown', { cancelable: true });
    Object.assign(event, { key: value, composedPath: () => [element] });
    document.dispatchEvent(event);
  };
  return { element, scaleHandle, rotHandle, dispose, pointer, key, captured, errors };
}
const writes = [];
let f = fixture(async display => writes.push(display));
f.element.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(100, 50));
paint();
assert.equal(f.element.style.left, '50%');
assert.equal(writes.length, 0, 'pointer movement never writes');
f.key('Escape');
assert.equal(f.element.style.left, '40%');
assert.equal(f.captured.size, 0, 'cancel releases pointer capture');
f.element.onpointerup();
assert.equal(writes.length, 0, 'cancelled pointerup does not commit');
f.key('ArrowRight');
await flush();
assert.equal(writes[0].xPercent, 41);
f.scaleHandle.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(10000));
f.element.onpointerup();
await flush();
assert.equal(writes[1].scale, 3, 'scale remains bounded');
f.rotHandle.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(20));
f.element.onpointercancel();
assert.equal(writes.length, 2);
assert.ok(f.element.style.transform.includes('rotate(0deg)'));
f.dispose();
f.key('ArrowRight');
assert.equal(writes.length, 2, 'disposed keyboard listener is removed');
assert.equal(f.element.onpointerdown, null);

let resolveWrite;
f = fixture(display => new Promise(resolve => { writes.push(display); resolveWrite = resolve; }));
f.element.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(80));
f.element.onpointerup();
f.element.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(200));
assert.equal(writes.length, 3, 'pending commit blocks another gesture');
f.dispose();
assert.equal(writes[2].xPercent, 48, 'teardown does not mutate the committed draft');
resolveWrite();
await flush();

f = fixture(async () => { throw Error('revision conflict'); });
f.element.onpointerdown(f.pointer(0));
f.element.onpointermove(f.pointer(80));
f.element.onpointerup();
await flush();
assert.equal(f.element.style.left, '40%', 'failed save rolls back');
assert.equal(f.errors[0].message, 'revision conflict');
f.dispose();
assert.equal(frames.size, 0);
console.log('Position gestures: cancellation, bounds, keyboard, pending ownership and cleanup passed');
