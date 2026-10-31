/**
 * Shared Host half of the Natives DSH plugins (ADR-0048).
 *
 * One library, used by all four plugin packages: it resolves the Core binary,
 * enforces the data-schema guard, obtains an authenticated page URL through the
 * existing `--launcher-url` handoff (ADR-0048 §影响), and serves that URL to its
 * own Client half over a same-origin read-only route.
 *
 * It never writes user data, never picks a port and never starts a second Core:
 * the URL comes from whatever Core is already running (attach) or from the one
 * Core the launcher handoff starts.
 */
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const TARGETS = new Set(['space', 'files', 'tokenusage', 'fund']);
export const THEMES = new Set(['archive', 'volt']);

const BINARY_CANDIDATES = ['hosts/native-file-host', 'native-file-host'];
const MANIFEST_CANDIDATES = ['product-manifest.json', 'product/manifest.json'];
/** Checked only inside a source checkout: `npm run extension:host:build`. */
const DEV_BINARY_CANDIDATES = ['target/debug/native-file-host', 'target/release/native-file-host'];
const LAUNCH_TIMEOUT_MS = 30_000;
/** ADR-0048 D6: the sealed payload carries a per-file sha256 manifest. */
export const PAYLOAD_MANIFEST = 'payload-manifest.json';
const PAYLOAD_MAX_FILES = 2_000;

/** Package root of the plugin that imports this copy of the runtime. */
export function pluginRoot() {
  return path.dirname(fileURLToPath(import.meta.url));
}

/** Repository root when this package is linked from a source checkout, else null. */
function checkoutRoot(dir) {
  const root = path.resolve(dir, '..', '..', '..');
  return exists(path.join(root, 'Cargo.toml')) ? root : null;
}

function firstExisting(dir, relatives) {
  for (const relative of relatives) {
    const candidate = path.join(dir, relative);
    if (exists(candidate)) return candidate;
  }
  return null;
}

function exists(file) {
  try {
    fs.accessSync(file, fs.constants.R_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Candidate Core state roots. `NATIVES_STATE_DIR` wins; otherwise both product
 * layouts are considered. The plugin never pins this for a spawned Core: a debug
 * Core writes `~/.natives-local` and a release Core writes `~/.natives`, so
 * forcing one would split or contaminate the other's data.
 */
export function stateDirs(env = process.env) {
  if (env.NATIVES_STATE_DIR) return [env.NATIVES_STATE_DIR];
  const home = env.HOME || os.homedir();
  return [path.join(home, '.natives'), path.join(home, '.natives-local')];
}

/** First existing state root, used only for read-only guard lookups. */
export function coreStateDir(env = process.env) {
  const dirs = stateDirs(env);
  return (
    dirs.find(
      (dir) =>
        exists(path.join(dir, 'natives.db')) || exists(path.join(dir, 'native-file-host.lock'))
    ) ?? dirs[0]
  );
}

/** Product sources in ADR-0048 D2 order: explicit override, dev source, system source. */
export function productSources(env = process.env) {
  const sources = [];
  if (env.NATIVES_DEV_PRODUCT_SOURCE) sources.push(env.NATIVES_DEV_PRODUCT_SOURCE);
  sources.push(path.join(env.HOME || os.homedir(), '.natives-local', 'product-source'));
  sources.push('/Library/Application Support/Natives');
  return sources;
}

/**
 * First usable Core, in this order:
 * 1. `NATIVES_CORE_BIN` (tests/advanced users);
 * 2. a source checkout's built binary (`target/debug|release`), so adding the
 *    plugin in the developer's own checkout needs no separate server;
 * 3. installed product sources (the user's product owns the data);
 * 4. the plugin's own sealed payload (`<plugin>/payload`), for machines with no
 *    Natives install.
 */
export function resolveCore({
  env = process.env,
  sources = productSources(env),
  pluginDir = pluginRoot(),
} = {}) {
  if (env.NATIVES_CORE_BIN) {
    return exists(env.NATIVES_CORE_BIN)
      ? { binary: env.NATIVES_CORE_BIN, source: null, override: true, mode: 'override' }
      : null;
  }

  const repo = checkoutRoot(pluginDir);
  if (repo) {
    const devBinary = firstExisting(repo, DEV_BINARY_CANDIDATES);
    if (devBinary) {
      const modelHost = path.join(path.dirname(devBinary), 'model-host');
      return {
        binary: devBinary,
        source: null,
        override: false,
        mode: 'dev',
        webRoot: path.join(repo, 'extension'),
        modelHost: exists(modelHost) ? modelHost : null,
      };
    }
  }

  for (const source of sources) {
    const candidate = firstExisting(source, BINARY_CANDIDATES);
    if (candidate) return { binary: candidate, source, override: false, mode: 'installed' };
  }

  const payload = path.join(pluginDir, 'payload');
  const payloadBinary = firstExisting(payload, BINARY_CANDIDATES);
  if (payloadBinary) {
    return { binary: payloadBinary, source: payload, override: false, mode: 'payload' };
  }
  return null;
}

export function readManifest(source) {
  if (!source) return null;
  for (const relative of MANIFEST_CANDIDATES) {
    const file = path.join(source, relative);
    if (!exists(file)) continue;
    try {
      return JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch {
      return null;
    }
  }
  return null;
}

/** `product_activation_path(apps_root)` — the parent of the apps root. */
export function activationPath(env = process.env) {
  if (env.NATIVES_APPS_ROOT) {
    return path.join(path.dirname(env.NATIVES_APPS_ROOT), 'product', 'activation.json');
  }
  const dirs = stateDirs(env);
  for (const dir of dirs) {
    const candidate = path.join(dir, 'product', 'activation.json');
    if (exists(candidate)) return candidate;
  }
  return path.join(dirs[0], 'product', 'activation.json');
}

export function readActivation(env = process.env) {
  const file = activationPath(env);
  if (!exists(file)) return null;
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

/**
 * ADR-0048 D7: a recorded `dataSchemaVersion` above the runtime manifest means
 * the user data was written by a newer module; refuse and ask for a plugin
 * upgrade instead of downgrading writes.
 */
export function checkSchema(activation, manifest) {
  const recorded = activation?.modules;
  if (!recorded || !manifest?.modules) return { ok: true };
  const runtime = new Map(
    manifest.modules.map((entry) => [entry.appId, entry.dataSchemaVersion])
  );
  for (const [appId, entry] of Object.entries(recorded)) {
    const want = entry?.dataSchemaVersion;
    const have = runtime.get(appId);
    if (Number.isInteger(want) && Number.isInteger(have) && want > have) {
      return { ok: false, code: 'schema-too-new', appId, recorded: want, runtime: have };
    }
  }
  return { ok: true };
}

function sha256File(file) {
  return createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

/**
 * ADR-0048 D6: verify the bundled payload's per-file sha256 manifest before
 * anything in it is spawned. The signed product manifest itself is verified by
 * the Core at startup; this check is the runtime-side integrity guard against
 * a corrupted or tampered bundle. A failed check refuses the launch.
 */
export function verifyPayload(payloadDir) {
  const manifestFile = path.join(payloadDir, PAYLOAD_MANIFEST);
  if (!exists(manifestFile)) return { ok: false, code: 'payload-manifest-missing' };
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
  } catch {
    return { ok: false, code: 'payload-manifest-invalid' };
  }
  const files = manifest?.files;
  if (!files || typeof files !== 'object' || Array.isArray(files)) {
    return { ok: false, code: 'payload-manifest-invalid' };
  }
  const entries = Object.entries(files);
  if (entries.length === 0 || entries.length > PAYLOAD_MAX_FILES) {
    return { ok: false, code: 'payload-manifest-invalid' };
  }
  for (const [relative, digest] of entries) {
    if (typeof digest !== 'string' || !/^[0-9a-f]{64}$/.test(digest)) {
      return { ok: false, code: 'payload-manifest-invalid', file: relative };
    }
    if (typeof relative !== 'string' || relative.length === 0) {
      return { ok: false, code: 'payload-manifest-invalid' };
    }
    // Reject traversal and absolute paths: only files inside the payload.
    const normalized = path.normalize(relative);
    if (normalized.startsWith('..') || path.isAbsolute(relative)) {
      return { ok: false, code: 'payload-path-escape', file: relative };
    }
    const file = path.join(payloadDir, normalized);
    if (!exists(file)) return { ok: false, code: 'payload-file-missing', file: relative };
    if (sha256File(file) !== digest) {
      return { ok: false, code: 'payload-file-mismatch', file: relative };
    }
  }
  return { ok: true, version: typeof manifest.version === 'string' ? manifest.version : null, files: entries.length };
}

function failure(code, message, extra = {}) {
  return Object.assign(new Error(message), { code, ...extra });
}

export function buildEnv({ env = process.env, source = null, modelHost = null, webRoot = null } = {}) {
  const child = { ...env };
  // Only an explicit override is passed through: the Core binary decides its own
  // default state root (debug → ~/.natives-local, release → ~/.natives).
  if (!env.NATIVES_STATE_DIR) delete child.NATIVES_STATE_DIR;
  if (source) child.NATIVES_DEV_PRODUCT_SOURCE = source;
  // A source checkout serves the working tree instead of the last packaged
  // product source, so `standalone`/theme changes are visible immediately.
  if (webRoot) child.NATIVES_WEB_ROOT = webRoot;
  else delete child.NATIVES_WEB_ROOT;
  // A source checkout has no packaged model-host next to the Core; point the
  // debug Core at the sibling build. Sealed payloads already carry it.
  if (modelHost) child.NATIVES_MODEL_HOST_BIN = modelHost;
  else delete child.NATIVES_MODEL_HOST_BIN;
  // Never inherit a stale binary override: the child is the Core, not a launcher
  // for a different build.
  delete child.NATIVES_CORE_BIN;
  return child;
}

export function runLauncher(binary, target, env, embedAncestors = null) {
  const args = ['--launcher-url', target];
  if (embedAncestors) args.push('--embed-ancestors', embedAncestors);
  return new Promise((resolve, reject) => {
    execFile(
      binary,
      args,
      { env, timeout: LAUNCH_TIMEOUT_MS, maxBuffer: 64 * 1024 },
      (error, stdout) => {
        if (error) {
          reject(failure('launcher-failed', error.message));
          return;
        }
        const line = String(stdout)
          .split('\n')
          .map((value) => value.trim())
          .filter(Boolean)
          .pop();
        try {
          const parsed = JSON.parse(line ?? '');
          if (typeof parsed.url !== 'string' || !parsed.url.startsWith('http://127.0.0.1:')) {
            reject(failure('launcher-failed', 'launcher returned an unexpected URL'));
            return;
          }
          resolve(parsed.url);
        } catch {
          reject(failure('launcher-failed', 'launcher did not return JSON'));
        }
      }
    );
  });
}

/** Authenticated page URL for one module, with the DSH appearance applied. */
export async function openModuleUrl({
  target,
  theme,
  embedAncestors = null,
  env = process.env,
  pluginDir = pluginRoot(),
} = {}) {
  if (!TARGETS.has(target)) throw failure('unknown-target', `unknown module: ${String(target)}`);
  const resolved = resolveCore({ env, pluginDir });
  if (!resolved) throw failure('core-missing', 'no Natives product source found');
  if (resolved.mode === 'payload') {
    const checked = verifyPayload(resolved.source);
    if (!checked.ok) {
      throw failure(
        checked.code,
        `bundled payload failed verification (${checked.code}${checked.file ? `: ${checked.file}` : ''})`,
        checked
      );
    }
  }
  const manifest = readManifest(resolved.source);
  const activation = readActivation(env);
  const gate = checkSchema(activation, manifest);
  if (!gate.ok) {
    throw failure(
      gate.code,
      `data schema ${String(gate.recorded)} (${gate.appId}) is newer than the bundled ${String(gate.runtime)}; upgrade the plugin`,
      gate
    );
  }
  const url = await runLauncher(
    resolved.binary,
    target,
    buildEnv({
      env,
      source: resolved.source,
      modelHost: resolved.modelHost,
      webRoot: resolved.webRoot,
    }),
    embedAncestors
  );
  return appendParam(appendParam(url, 'theme', THEMES.has(theme) ? theme : 'archive'), 'standalone', '1');
}

/**
 * Appends one query parameter without re-serializing the query: the launcher
 * already percent-encoded the ancestors (`%20` between sources), and a
 * `URLSearchParams` round-trip would rewrite that to `+`, breaking the
 * whitespace-separated ancestor list the Core decodes.
 */
function appendParam(url, key, value) {
  const hash = url.indexOf('#');
  const base = hash >= 0 ? url.slice(0, hash) : url;
  const fragment = hash >= 0 ? url.slice(hash) : '';
  const separator = base.includes('?') ? '&' : '?';
  return `${base}${separator}${key}=${encodeURIComponent(value)}${fragment}`;
}

function sameOrigin(request) {
  const origin = request.headers.origin;
  const site = request.headers['sec-fetch-site'];
  if (site === 'cross-site') return false;
  if (!origin) return true;
  const host = request.headers.host;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

function reply(response, status, payload) {
  const body = JSON.stringify(payload);
  response.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'no-store',
    'Content-Length': Buffer.byteLength(body),
  });
  response.end(body);
}

/**
 * Serve `GET /natives/<module>/url?theme=…` → `{ url }`.
 *
 * Read-only, same-origin, no caching: it returns a short-lived authenticated URL
 * and never proxies the page, forwards business data or mutates state.
 */
export function registerUrlRoute(ctx, { route, target }) {
  // The desktop window renders on the measured `dsh-app://app` origin; the web
  // profile is a loopback http(s) origin. Both DSH scheme sources are minted so
  // either host shape frames the page; the loopback origin is appended when
  // the host reports one.
  const origin = ctx.webServer.getOrigin?.();
  const schemeSources = 'dsh-app: dsh:';
  const embedAncestors = origin ? `${schemeSources} ${origin}` : schemeSources;
  ctx.effect(
    () =>
      ctx.webServer.register({
        kind: 'exact',
        path: route,
        handler: (request, response) => {
          if (!sameOrigin(request)) {
            reply(response, 403, { code: 'cross-origin', message: 'same-origin only' });
            return;
          }
          let theme = 'archive';
          try {
            const requested = new URL(request.url, 'http://127.0.0.1').searchParams.get('theme');
            if (THEMES.has(requested)) theme = requested;
          } catch {
            /* default theme */
          }
          openModuleUrl({ target, theme, embedAncestors }).then(
            (url) => reply(response, 200, { url }),
            (error) =>
              reply(response, error.code === 'schema-too-new' ? 409 : 503, {
                code: error.code ?? 'failed',
                message: error.message,
                appId: error.appId,
                recorded: error.recorded,
                runtime: error.runtime,
              })
          );
        },
      }),
    `natives:${target}:url-route`
  );
}
