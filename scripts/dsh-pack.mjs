#!/usr/bin/env node
// One-command packaging for the four DSH plugins (ADR-0048).
//
//   node scripts/dsh-pack.mjs                    # four payload-less tarballs
//   node scripts/dsh-pack.mjs --source <dir>     # seal payloads first, then pack
//
// Safety: without `--source`, any leftover `payload/` directory inside a plugin
// package REFUSES the pack — a dev-trust-root payload must never ship silently
// (交付纪律；payload 分发只接受生产签名产品源，见 ADR-0048 D6)。
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const PLUGINS = ['space', 'files', 'tokenusage', 'fund'];
const DIST = join(ROOT, 'integrations', 'dsh', 'dist');

const args = process.argv.slice(2);
const sourceIndex = args.indexOf('--source');
if (sourceIndex >= 0) {
  const source = args[sourceIndex + 1];
  if (!source) {
    console.error('--source requires a product source directory');
    process.exit(2);
  }
  execFileSync('node', ['scripts/dsh-payload.mjs', '--source', source], { stdio: 'inherit', cwd: ROOT });
} else {
  const leftovers = PLUGINS.filter((plugin) => existsSync(join(ROOT, 'integrations', 'dsh', plugin, 'payload')));
  if (leftovers.length) {
    console.error(`refusing to pack: payload/ present in ${leftovers.join(', ')} without --source.`);
    console.error('pass --source <complete-product-source> to seal intentionally, or remove the payload directories.');
    process.exit(1);
  }
}

rmSync(DIST, { recursive: true, force: true });
mkdirSync(DIST, { recursive: true });
for (const plugin of PLUGINS) {
  execFileSync('npm', ['pack', '--pack-destination', DIST], {
    cwd: join(ROOT, 'integrations', 'dsh', plugin),
    stdio: 'pipe',
  });
  console.log(`✓ packed @natives/dsh-${plugin}`);
}
console.log(`tarballs in ${DIST}`);
