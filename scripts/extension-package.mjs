// All extension distributors and budget checks consume these exact bytes.
import { existsSync, readdirSync, readFileSync, lstatSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { transformSync } from 'esbuild';

export const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const OMIT = new Set(['node_modules', '.git', 'fixtures', 'test', 'tests', '__pycache__']);
const FILES = /^(manifest\.json|(?!(?:ui-harness|test-dom-mock|files-preview)\.js$)(?:[^/]+|plugins\/.+|apps\/.+)\.(?:html|css|js)|apps\/catalog-v[12]\.(?:json|sig)|icons\/(?!folder-source\.svg$)[^/]+|_locales\/[^/]+\/messages\.json)$/;
const EXECUTABLE_MAGIC = new Set(['7f454c46', 'cffaedfe', 'cefaedfe', 'feedfacf', 'feedface', 'cafebabe', 'bebafeca']);

export function distributableFiles(root = ROOT) {
  const base = join(root, 'extension'), files = [];
  if (!existsSync(base)) return files;
  function visit(dir) {
    for (const name of readdirSync(dir)) {
      if (OMIT.has(name)) continue;
      const path = join(dir, name), info = lstatSync(path);
      if (info.isSymbolicLink()) throw new Error('Linked extension asset: ' + relative(base, path));
      if (info.isDirectory()) { visit(path); continue; }
      const file = relative(base, path).split('\\').join('/');
      const bytes = readFileSync(path);
      if (/\.(nap|exe|dll|so|dylib|node|wasm|zip)$/i.test(file)
          || EXECUTABLE_MAGIC.has(bytes.subarray(0, 4).toString('hex')) || bytes.subarray(0, 2).toString() === 'MZ') {
        throw new Error('Native executable or package in extension: ' + file);
      }
      if (FILES.test(file) && !file.split('/').some((part) => part.startsWith('.'))) files.push(file);
    }
  }
  visit(base);
  return files.sort();
}

export function extensionFileBytes(file, root = ROOT) {
  const bytes = readFileSync(join(root, 'extension', file));
  const loader = file.endsWith('.js') ? 'js' : file.endsWith('.css') ? 'css' : null;
  if (loader) return Buffer.from(transformSync(bytes.toString('utf8'), {
    loader, minify: true, target: 'chrome120', legalComments: 'none', charset: 'utf8',
  }).code);
  if (file === 'manifest.json' || file.startsWith('_locales/')) return Buffer.from(JSON.stringify(JSON.parse(bytes)));
  // The signed catalog must retain its exact byte representation.
  return bytes;
}

export function buildExtension(destination = join(ROOT, 'dist/extension'), root = ROOT) {
  const files = distributableFiles(root);
  const output = files.map((file) => [file, extensionFileBytes(file, root)]);
  rmSync(destination, { recursive: true, force: true });
  for (const [file, bytes] of output) {
    const path = join(destination, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, bytes);
  }
  return files;
}

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const files = buildExtension();
  console.log(JSON.stringify({ output: join(ROOT, 'dist/extension'), files: files.length }));
}
