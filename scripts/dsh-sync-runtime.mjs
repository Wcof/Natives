// Copy the shared DSH host runtime into each plugin package.
//
// Installers link a local package directory, so a workspace dependency such as
// `@natives/dsh-runtime` would not be installed for another user. Each plugin
// therefore ships its own generated copy; this script keeps them byte-identical
// to `integrations/dsh/runtime/index.js`, and `--check` fails on drift.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const SOURCE = join(ROOT, 'integrations/dsh/runtime/index.js');
const PLUGINS = ['space', 'files', 'tokenusage', 'fund'];

export function syncRuntime({ check = false } = {}) {
  const source = readFileSync(SOURCE, 'utf8');
  const drifted = [];
  for (const plugin of PLUGINS) {
    const target = join(ROOT, 'integrations/dsh', plugin, 'runtime.js');
    if (check) {
      if (!existsSync(target) || readFileSync(target, 'utf8') !== source) drifted.push(plugin);
      continue;
    }
    writeFileSync(target, source);
  }
  return drifted;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes('--check');
  const drifted = syncRuntime({ check });
  if (drifted.length > 0) {
    console.error(`dsh runtime drift: ${drifted.join(', ')}; run node scripts/dsh-sync-runtime.mjs`);
    process.exit(1);
  }
  console.log(check ? 'dsh runtime copies match' : 'dsh runtime copies written');
}
