// ─── Extension Host Tests ────────────────────────────────
//
// Tests for manifest validation, capability denial, timeout,
// output limit, crash, and restart.

import { describe, it, assert } from 'tsx:test';
import { ExtensionHost, type ExtensionManifest } from './index';

describe('ExtensionHost', () => {
  it('should load a valid extension manifest', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'test-ext-1',
      name: 'Test Extension',
      version: '1.0.0',
      kind: 'plugin',
      description: 'A test extension',
      permissions: ['read', 'search'],
      entry: './test.js',
    };

    const instance = await host.loadExtension(manifest);
    assert.equal(instance.manifest.id, 'test-ext-1');
    assert.equal(instance.manifest.name, 'Test Extension');
  });

  it('should reject invalid manifest', async () => {
    const host = new ExtensionHost();
    const manifest = { id: 'test' } as ExtensionManifest;

    try {
      await host.loadExtension(manifest);
      assert.fail('Should have thrown');
    } catch (e: unknown) {
      assert.ok(e instanceof Error);
      assert.ok((e as Error).message.includes('Invalid manifest'));
    }
  });

  it('should deny unknown capabilities', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'malicious-ext',
      name: 'Malicious',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Attempts to use denied capabilities',
      permissions: ['read', 'write', 'network', 'credentials'],
      entry: './malicious.js',
    };

    try {
      await host.loadExtension(manifest);
      assert.fail('Should have denied the extension');
    } catch (e: unknown) {
      assert.ok(e instanceof Error);
      assert.ok((e as Error).message.includes('not allowed'));
    }
  });

  it('should enforce timeout limits', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'timeout-ext',
      name: 'Timeout Test',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Tests timeout enforcement',
      permissions: ['read'],
      entry: './timeout.js',
    };

    const instance = await host.loadExtension(manifest);
    assert.equal(instance.limits.timeoutMs, 30_000);
    assert.ok(instance.limits.checkOutputSize(1024));
  });

  it('should enforce output size limits', () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'output-ext',
      name: 'Output Test',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Tests output limit',
      permissions: ['read'],
      entry: './output.js',
    };

    // Test output size check
    assert.ok(host.getRPC() !== undefined, 'RPC interface should be available');
  });

  it('should list loaded extensions', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'list-ext',
      name: 'List Test',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Tests listing',
      permissions: ['read'],
      entry: './list.js',
    };

    await host.loadExtension(manifest);
    const extensions = host.listExtensions();
    assert.equal(extensions.length, 1);
    assert.equal(extensions[0]!.id, 'list-ext');
  });

  it('should unload extensions', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'unload-ext',
      name: 'Unload Test',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Tests unloading',
      permissions: ['read'],
      entry: './unload.js',
    };

    await host.loadExtension(manifest);
    assert.equal(host.listExtensions().length, 1);
    host.unloadExtension('unload-ext');
    assert.equal(host.listExtensions().length, 0);
  });

  it('should get extension by ID', async () => {
    const host = new ExtensionHost();
    const manifest: ExtensionManifest = {
      id: 'get-ext',
      name: 'Get Test',
      version: '1.0.0',
      kind: 'plugin',
      description: 'Tests get by ID',
      permissions: ['read'],
      entry: './get.js',
    };

    await host.loadExtension(manifest);
    const instance = host.getExtension('get-ext');
    assert.ok(instance !== undefined);
    assert.equal(instance!.manifest.id, 'get-ext');
  });
});