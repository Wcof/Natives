import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import { validateManifest, checkCompatibility, scanModules, syncModulesToDb } from './module-manager';

const TEST_MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');

describe('ModuleManager', () => {
  const testModuleDir = path.join(TEST_MODULES_DIR, 'test-module');

  before(() => {
    // Clean and create test module directory
    if (fs.existsSync(TEST_MODULES_DIR)) {
      fs.rmSync(TEST_MODULES_DIR, { recursive: true });
    }
    fs.mkdirSync(testModuleDir, { recursive: true });
  });

  after(() => {
    if (fs.existsSync(TEST_MODULES_DIR)) {
      fs.rmSync(TEST_MODULES_DIR, { recursive: true });
    }
  });

  describe('validateManifest', () => {
    it('should accept a valid manifest', () => {
      const result = validateManifest({
        id: 'com.example.app',
        name: 'Example App',
        version: '1.0.0',
        entry: 'index.html',
        type: 'web',
      });
      assert.equal(result.ok, true);
      if (result.ok) {
        assert.equal(result.manifest.id, 'com.example.app');
        assert.equal(result.manifest.type, 'web');
      }
    });

    it('should reject manifest missing required fields', () => {
      const result = validateManifest({
        name: 'Missing ID',
      });
      assert.equal(result.ok, false);
      if (!result.ok) {
        assert.ok(result.error.includes('id'));
      }
    });

    it('should accept manifest with all optional fields', () => {
      const result = validateManifest({
        id: 'com.example.full',
        name: 'Full App',
        version: '2.0.0',
        entry: 'app.html',
        type: 'web',
        description: 'A full example',
        author: 'Test',
        icon: 'icon.svg',
        minNativesVersion: '0.1.0',
        permissions: ['db:read', 'notification'],
      });
      assert.equal(result.ok, true);
      if (result.ok) {
        assert.equal(result.manifest.permissions.length, 2);
        assert.equal(result.manifest.minNativesVersion, '0.1.0');
      }
    });

    it('should reject invalid type', () => {
      const result = validateManifest({
        id: 'x',
        name: 'x',
        version: '1.0.0',
        entry: 'x',
        type: 'invalid-type',
      });
      assert.equal(result.ok, false);
    });
  });

  describe('checkCompatibility', () => {
    it('should be compatible when no version specified', () => {
      const result = checkCompatibility(undefined);
      assert.equal(result.compatible, true);
    });

    it('should be compatible when version matches current', () => {
      const result = checkCompatibility('0.1.0');
      assert.equal(result.compatible, true);
    });

    it('should be incompatible on major version mismatch', () => {
      const result = checkCompatibility('1.0.0');
      assert.equal(result.compatible, false);
      assert.ok(result.warning);
    });

    it('should warn on minor version mismatch', () => {
      const result = checkCompatibility('0.2.0');
      assert.equal(result.compatible, true);
      assert.ok(result.warning);
    });
  });

  describe('scanModules', () => {
    it('should find and validate a module directory', () => {
      // Write valid manifest
      fs.writeFileSync(
        path.join(testModuleDir, 'manifest.json'),
        JSON.stringify({
          id: 'test-module',
          name: 'Test Module',
          version: '1.0.0',
          entry: 'index.html',
          type: 'web',
        }),
        'utf-8',
      );
      fs.writeFileSync(path.join(testModuleDir, 'index.html'), '<h1>Test</h1>', 'utf-8');

      const results = scanModules();
      assert.equal(results.length, 1);
      assert.equal(results[0]!.moduleId, 'test-module');
      assert.ok(results[0]!.manifest);
      assert.equal(results[0]!.manifest!.name, 'Test Module');
    });

    it('should report error for directory without manifest', () => {
      const dirNoManifest = path.join(TEST_MODULES_DIR, 'no-manifest');
      fs.mkdirSync(dirNoManifest, { recursive: true });

      const results = scanModules();
      const noManifest = results.find((r) => r.moduleId === 'no-manifest');
      assert.ok(noManifest);
      assert.equal(noManifest!.manifest, null);
      assert.ok(noManifest!.error);
    });

    it('should report error for invalid manifest JSON', () => {
      const dirBadJson = path.join(TEST_MODULES_DIR, 'bad-json');
      fs.mkdirSync(dirBadJson, { recursive: true });
      fs.writeFileSync(path.join(dirBadJson, 'manifest.json'), '{broken json', 'utf-8');

      const results = scanModules();
      const badJson = results.find((r) => r.moduleId === 'bad-json');
      assert.ok(badJson);
      assert.ok(badJson!.error);
    });
  });
});
