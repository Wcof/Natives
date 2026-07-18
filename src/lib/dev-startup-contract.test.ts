import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

const root = new URL('../../', import.meta.url);
const pkg = JSON.parse(readFileSync(new URL('package.json', root), 'utf8'));
const tauri = JSON.parse(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8'));
const bundle = JSON.parse(
  readFileSync(new URL('src-tauri/tauri.bundle.conf.json', root), 'utf8'),
);

describe('desktop development startup', () => {
  it('uses one desktop entrypoint and a renderer-only Tauri hook', () => {
    assert.match(pkg.scripts.dev, /daemon:build/);
    assert.match(pkg.scripts.dev, /tauri dev/);
    assert.match(pkg.scripts['web:dev'], /next dev/);
    assert.equal(tauri.build.beforeDevCommand, 'npm run web:dev');
  });

  it('bundles the Agent Daemon for production desktop builds', () => {
    assert.match(pkg.scripts['daemon:bundle'], /prepare-sidecar/);
    assert.match(pkg.scripts['tauri:build'], /daemon:bundle.*tauri build.*tauri\.bundle\.conf/);
    assert.deepEqual(bundle.bundle.externalBin, ['binaries/natives-agent-daemon']);
  });
});
