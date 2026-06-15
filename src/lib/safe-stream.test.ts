import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { wrapController } from './safe-stream';

describe('SafeStream', () => {
  function createMockController() {
    const chunks: unknown[] = [];
    let closed = false;
    return {
      raw: {
        enqueue(chunk: unknown) { if (closed) throw new Error('Invalid state: Controller is already closed'); chunks.push(chunk); },
        close() { if (closed) throw new Error('Invalid state: Controller is already closed'); closed = true; },
        error() { closed = true; },
      },
      chunks,
      isClosed: () => closed,
    };
  }

  it('should enqueue chunks normally', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.enqueue('hello');
    ctrl.enqueue('world');
    assert.deepEqual(mock.chunks, ['hello', 'world']);
    assert.equal(ctrl.closed, false);
  });

  it('should close normally', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.close();
    assert.equal(ctrl.closed, true);
  });

  it('should silently swallow close errors on enqueue', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.close(); // Close first
    // Enqueue after close should not throw
    ctrl.enqueue('after-close');
    assert.equal(ctrl.closed, true);
  });

  it('should call onClosedWrite callback once', () => {
    const mock = createMockController();
    let callbackCount = 0;
    const ctrl = wrapController(
      mock.raw as unknown as ReadableStreamDefaultController<string>,
      () => { callbackCount++; },
    );
    ctrl.close();
    ctrl.enqueue('after-close'); // Should trigger callback
    ctrl.enqueue('again'); // Should NOT trigger callback again
    assert.equal(callbackCount, 1);
  });

  it('should handle error without throwing', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.error(new Error('test error'));
    assert.equal(ctrl.closed, true);
  });

  it('should not enqueue after error', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.error(new Error('test'));
    ctrl.enqueue('after-error');
    assert.deepEqual(mock.chunks, []);
  });

  it('should not close twice', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.close();
    ctrl.close(); // Should not throw
    assert.equal(ctrl.closed, true);
  });

  it('should not error after close', () => {
    const mock = createMockController();
    const ctrl = wrapController(mock.raw as unknown as ReadableStreamDefaultController<string>);
    ctrl.close();
    ctrl.error(new Error('late error')); // Should not throw
    assert.equal(ctrl.closed, true);
  });
});
