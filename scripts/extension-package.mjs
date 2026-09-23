// All extension distributors and budget checks consume these exact bytes.
import { existsSync, readdirSync, readFileSync, lstatSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { transformSync } from 'esbuild';

export const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const OMIT = new Set(['node_modules', '.git', 'fixtures', 'test', 'tests', '__pycache__']);
const FILES = /^(manifest\.json|(?!(?:ui-harness|test-dom-mock|files-preview)\.js$)(?:[^/]+|plugins\/.+|apps\/.+|ai-performance\/.+|tokenusage\/.+)\.(?:html|css|js)|icons\/(?:agents\/)?(?!folder-source\.svg$)[^/]+|_locales\/[^/]+\/messages\.json)$/;

// 生产 Extension 身份稳定化（本地加载已解压模式）：
// manifest.json 注入固定 RSA 公钥（key 字段），Chrome 由此派生稳定 Extension ID，
// 与加载路径无关；ID = SHA-256(SPKI DER) 前 16 字节映射 a–p。
// 派生脚本一次性生成，密钥材料随仓库受版本控制，任何 Release 不得更换。
// 模式隔离（方案 §5 P1/契约 §4.2）：local 候选使用独立 key/ID，绝不与
// production 共用 Extension ID——两种模式可并存且互不覆盖。
const EXTENSION_ID_KEY = readFileSync(join(ROOT, 'scripts/extension-id-key.b64'), 'utf8').trim();
const EXTENSION_ID_KEY_LOCAL = readFileSync(join(ROOT, 'scripts/extension-id-key.local.b64'), 'utf8').trim();

function extensionIdForKey(keyB64) {
  const hash = createHash('sha256').update(keyB64, 'base64').digest();
  return [...hash.subarray(0, 16)]
    .map((b) => (b >> 4).toString(16) + (b & 15).toString(16))
    .map((h) => h.split('').map((d) => String.fromCharCode(97 + parseInt(d, 16))).join(''))
    .join('');
}

export const STABLE_EXTENSION_ID = extensionIdForKey(EXTENSION_ID_KEY);
export const LOCAL_EXTENSION_ID = extensionIdForKey(EXTENSION_ID_KEY_LOCAL);

// local 模式的 Native Messaging Host 名称（与 build-pkg.sh 的注册文件名一致）。
// esbuild minify 之后字符串字面量通常输出为双引号，因此同时兼容替换单/双引号及无引号标识符。
const HOST_NAME_SUBSTITUTIONS_LOCAL = [
  ['"com.natives.file_manager"', '"com.natives.local.file_manager"'],
  ["'com.natives.file_manager'", "'com.natives.local.file_manager'"],
  ['"com.natives.model_host"', '"com.natives.local.model_host"'],
  ["'com.natives.model_host'", "'com.natives.local.model_host'"],
];

function extensionKeyFor(mode) {
  return mode === 'local' ? EXTENSION_ID_KEY_LOCAL : EXTENSION_ID_KEY;
}

function validateManifest(manifest, root) {
  if (manifest.side_panel) {
    throw new Error(`Invalid manifest: side_panel is not supported (${JSON.stringify(manifest.side_panel)})`);
  }
  const extDir = join(root, 'extension');
  const declaredPaths = [
    manifest.chrome_url_overrides?.newtab,
    manifest.background?.service_worker,
    ...Object.values(manifest.action?.default_icon || {}),
    ...Object.values(manifest.icons || {}),
    ...(manifest.content_scripts || []).flatMap((cs) => [...(cs.js || []), ...(cs.css || [])]),
  ].filter(Boolean);
  for (const relPath of declaredPaths) {
    if (!existsSync(join(extDir, relPath))) {
      throw new Error(`Manifest referenced file does not exist: ${join(extDir, relPath)}`);
    }
  }
}

function manifestBytes(root, mode = 'production') {
  const manifest = JSON.parse(readFileSync(join(root, 'extension/manifest.json'), 'utf8'));
  validateManifest(manifest, root);
  manifest.key = extensionKeyFor(mode);
  return Buffer.from(JSON.stringify(manifest));
}
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
  if (file.endsWith('.svg')) {
    // SVG 无构建期压缩通道；去除注释与标签间空白（不改动矢量语义）。
    const minified = bytes.toString('utf8')
      .replace(/<!--[\s\S]*?-->/g, '')
      .replace(/>\s+</g, '><')
      .replace(/\s{2,}/g, ' ')
      .trim();
    return Buffer.from(minified);
  }
  // Static assets retain their exact byte representation.
  return bytes;
}

export function buildExtension(destination = join(ROOT, 'dist/extension'), root = ROOT, mode = 'production') {
  const local = mode === 'local';
  const files = distributableFiles(root);
  const output = files.map((file) => {
    let bytes = file === 'manifest.json' ? manifestBytes(root, mode) : extensionFileBytes(file, root);
    // local 模式隔离：JS 内的 Core/Model Host 名替换为 local 命名空间，
    // 与 build-pkg.sh 的 NM 注册文件名严格一致（plan §5 P1）。
    if (local && file.endsWith('.js')) {
      let text = bytes.toString('utf8');
      for (const [from, to] of HOST_NAME_SUBSTITUTIONS_LOCAL) text = text.split(from).join(to);
      bytes = Buffer.from(text, 'utf8');
    }
    return [file, bytes];
  });
  rmSync(destination, { recursive: true, force: true });
  for (const [file, bytes] of output) {
    const path = join(destination, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, bytes);
  }
  return files;
}

// The product ships this fixed directory inside the complete Natives installer;
// there is no extension-only release artifact or runtime unpack step.

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const manifest = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8'));
  const destination = join(ROOT, 'dist/extension');
  const files = buildExtension(destination, ROOT, 'production');
  console.log(JSON.stringify({ destination, extensionId: STABLE_EXTENSION_ID, version: manifest.version, fileCount: files.length }));
}
