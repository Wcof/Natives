#!/usr/bin/env node
/**
 * Natives zero-dependency architecture gate.
 *
 * Checks (each violation maps to a debt-manifest key `file:line` or `file`):
 *   1. over_1000            handwritten source files > 1,000 lines
 *   2. over_700_ledger      files > 700 lines (review ledger, warning only)
 *   3. ui_business          components/ui importing business modules
 *   4. feature_cross        feature importing another feature's internals
 *   5. thick_page           src/app page.tsx with thick logic
 *   6. raw_invoke           bare invoke() outside src/lib/tauri
 *   7. native_dialog        alert()/prompt()/confirm() in src
 *   8. cross_db_daemon      daemon code paths touching Host natives.db
 *   9. cross_db_host        host code paths touching Daemon assistant.db
 *  10. embedded_prod        EmbeddedAuthority in production compile surface
 *  11. global_singleton     new static mut / OnceLock / lazy_static
 *  12. unregistered_interval setInterval in src without ledger entry (warning)
 *  13. hooks_reverse        src/hooks importing @/components
 *  14. budget_functions     heuristic fn bodies > 120 lines (warning)
 *
 * Debt manifest: scripts/architecture-debt-manifest.json. `--baseline`
 * snapshots all current violations so the gate passes while remediation is in
 * flight; afterwards any NEW violation (not in the manifest) fails the gate.
 * Warnings (over_700_ledger / unregistered_interval / budget_functions) never
 * fail; they only produce a review ledger. The manifest must be emptied by the
 * end of the remediation — no permanent exemptions.
 *
 * Usage:
 *   node scripts/architecture-check.mjs [--baseline]
 */

import { readFileSync, readdirSync, statSync, existsSync, writeFileSync } from 'fs';
import { resolve, dirname, relative, join, sep } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';
import { createRequire } from 'module';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const MANIFEST = join(__dirname, 'architecture-debt-manifest.json');
const BASELINE = process.argv.includes('--baseline');

// typescript is resolved lazily: local node_modules first, then the main
// workspace's node_modules (worktrees never install their own node_modules).
const requireLocal = createRequire(import.meta.url);
let tsModule = null;
function loadTypeScript() {
  if (tsModule) return tsModule;
  const candidates = ['typescript', join(ROOT, 'node_modules', 'typescript')];
  // Worktree fallback: the main workspace lives next to this worktree.
  const mainWs = join(ROOT, '..', 'Natives', 'node_modules', 'typescript');
  if (mainWs !== join(ROOT, 'node_modules', 'typescript')) candidates.push(mainWs);
  for (const c of candidates) {
    try {
      tsModule = requireLocal(c);
      return tsModule;
    } catch {
      /* try next */
    }
  }
  throw new Error('typescript package unavailable (needed for AST checks)');
}

// ---------------------------------------------------------------------------
// Scan helpers
// ---------------------------------------------------------------------------

const SKIP_RE =
  /(^|\/)(node_modules|target|\.next|\.git|coverage|dist|out|\.runtime-evidence|archive|build)(\/|$)/;
const GENERATED_RE = /(^|\/)(generated|gen)(\/|$)/;

function walk(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      if (SKIP_RE.test(p) || GENERATED_RE.test(p)) continue;
      walk(p, out);
    } else if (e.isFile()) {
      out.push(p);
    }
  }
  return out;
}

function isHandwrittenTs(p) {
  return /\.(ts|tsx)$/.test(p) && !/\.test\.(ts|tsx)$/.test(p) && !p.includes('src/types/generated');
}

function isTestFile(p) {
  return /\.test\.(ts|tsx|rs)$/.test(p) || /(\/|^)tests?(\/|\.)/.test(p) || /_tests?\.rs$/.test(p);
}

function countLines(p) {
  try {
    return readFileSync(p, 'utf8').split('\n').length;
  } catch {
    return 0;
  }
}

function nonCommentLines(p, re) {
  const hits = [];
  const lines = readFileSync(p, 'utf8').split('\n');
  lines.forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('//') || t.startsWith('*') || t.startsWith('/*') || t.startsWith('#!')) return;
    if (re.test(t)) hits.push({ line: i + 1, text: t.slice(0, 110) });
  });
  return hits;
}

function importsOf(p, src) {
  const out = [];
  const re = /from\s+['"]([^'"]+)['"]/g;
  let m;
  while ((m = re.exec(src)) !== null) out.push(m[1]);
  return out;
}

function relToRoot(p) {
  return relative(ROOT, p).split(sep).join('/');
}

// ---------------------------------------------------------------------------
// Collectors (return Map<key, reason>)
// ---------------------------------------------------------------------------

function collectOver1000(root = ROOT) {
  const map = new Map();
  const roots = [
    join(root, 'src'),
    join(root, 'src-tauri/src'),
    join(root, 'src-tauri/tests'),
    join(root, 'src-agent-daemon/src'),
    join(root, 'src-agent-daemon/tests'),
    join(root, 'crates'),
    join(root, 'extension-host'),
    join(root, 'scripts'),
  ];
  for (const r of roots) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!/\.(ts|tsx|css|rs|mjs)$/.test(p)) continue;
      if (GENERATED_RE.test(p)) continue;
      const n = countLines(p);
      if (n > 1000) map.set(relToRoot(p), `${n} lines`);
    }
  }
  return map;
}

function collectOver700(root = ROOT) {
  const map = new Map();
  for (const r of [join(root, 'src'), join(root, 'src-tauri/src'), join(root, 'src-agent-daemon/src'), join(root, 'crates')]) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!/\.(ts|tsx|css|rs)$/.test(p)) continue;
      const n = countLines(p);
      if (n > 700 && n <= 1000) map.set(relToRoot(p), `${n} lines (review ledger)`);
    }
  }
  return map;
}

function collectUiBusiness(root = ROOT) {
  const map = new Map();
  const uiRoot = join(root, 'src/components/ui');
  if (!existsSync(uiRoot)) return map;
  // UI atoms may only import: external packages, other ui files, design-tokens,
  // i18n (framework), pure type files, and generic UI infrastructure libs
  // (focus trap / markdown safety / toast context — no business logic).
  const LIB_ALLOW = ['design-tokens', 'i18n', 'types/', 'useFocusTrap', 'markdown-safety', 'toast-context'];
  for (const p of walk(uiRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (/^['"]/.test(spec) && !spec.startsWith('@/') && !spec.startsWith('.')) continue; // external
      if (spec.startsWith('@/components/ui')) continue; // sibling ui
      if (spec.startsWith('@/lib/')) {
        const rest = spec.slice('@/lib/'.length);
        if (LIB_ALLOW.some((a) => rest.startsWith(a))) continue;
        map.set(`${relToRoot(p)}:${spec}`, `ui imports business lib ${spec}`);
      } else if (spec.startsWith('@/components/')) {
        map.set(`${relToRoot(p)}:${spec}`, `ui imports business component ${spec}`);
      } else if (spec.startsWith('.')) {
        const target = resolve(p, '..', spec);
        if (target.startsWith(uiRoot)) continue;
        map.set(`${relToRoot(p)}:${spec}`, `ui imports relative business module ${spec}`);
      }
    }
  }
  return map;
}

function collectFeatureCross(root = ROOT) {
  const map = new Map();
  const compRoot = join(root, 'src/components');
  if (!existsSync(compRoot)) return map;
  // Shared business domains that multiple features legitimately reuse:
  // ui (atoms), shell, preview, capabilities, and the assistant conversation /
  // diff subdomains (conversation UI + diff viewer are cross-feature shared
  // components after W4's components/ui extraction). Importing those is not a
  // horizontal feature import; importing another feature's private internals is.
  const SHARED_DOMAINS = new Set(['ui', 'shell', 'preview', 'capabilities']);
  for (const p of walk(compRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    const m = /^src\/components\/([^/]+)\//.exec(rel);
    if (!m) continue;
    const myDomain = m[1];
    if (SHARED_DOMAINS.has(myDomain)) continue;
    if (myDomain === 'assistant') {
      // Assistant workspace is the shared conversation/activity owner; other
      // features may reuse its conversation/diff shared components, but not
      // arbitrary assistant internals.
      const sub = /^src\/components\/assistant\/([^/]+)\//.exec(rel);
      if (sub && SHARED_ASSISTANT_SUBDOMAINS.has(sub[1])) continue;
    }
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (!spec.startsWith('@/components/')) continue;
      const target = spec.slice('@/components/'.length).split('/')[0];
      if (target === myDomain || SHARED_DOMAINS.has(target)) continue;
      if (target === 'assistant') {
        const sub = spec.slice('@/components/'.length).split('/')[1];
        if (sub && SHARED_ASSISTANT_SUBDOMAINS.has(sub)) continue;
      }
      map.set(`${rel}:${spec}`, `feature ${myDomain} imports ${target} internals`);
    }
  }
  return map;
}

const SHARED_ASSISTANT_SUBDOMAINS = new Set(['conversation', 'diff']);

function collectThickPages(root = ROOT) {
  const map = new Map();
  const appRoot = join(root, 'src/app');
  if (!existsSync(appRoot)) return map;
  for (const p of walk(appRoot)) {
    if (!p.endsWith('page.tsx') && !p.endsWith('page.ts')) continue;
    const n = countLines(p);
    if (n > 80) map.set(relToRoot(p), `${n} lines (page should stay thin)`);
  }
  return map;
}

function collectRawInvoke(root = ROOT) {
  const map = new Map();
  for (const p of walk(join(root, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    if (rel.startsWith('src/lib/tauri/') || rel.startsWith('src/lib/tauri-adapter')) continue;
    for (const h of nonCommentLines(p, /(^|[^\w.])\binvoke\s*\(/)) {
      map.set(`${rel}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectNativeDialog(root = ROOT) {
  const map = new Map();
  for (const p of walk(join(root, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    for (const h of nonCommentLines(p, /(^|[^\w.])(alert|prompt|confirm)\s*\(/)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectCrossDbDaemon(root = ROOT) {
  const map = new Map();
  const daemonRoot = join(root, 'src-agent-daemon/src');
  if (!existsSync(daemonRoot)) return map;
  for (const p of walk(daemonRoot)) {
    if (!p.endsWith('.rs')) continue;
    if (isTestFile(p)) continue;
    // Only flag an actual DB-open into the Host-authoritative natives.db:
    //   Connection::open / open_with_flags whose target is default_natives_db_path
    //   or the `natives.db` literal. Broker lease clients (`NativesDbBroker::*`,
    //   `open_default`) never open the file, fail-closed `write_setting` never
    //   writes it, and `default_assistant_db_path` is the daemon's own store.
    const re = /(Connection::open|open_with_flags)\([^)]*(default_natives_db_path|join\("natives\.db"\)|"natives\.db")/;
    for (const h of nonCommentLines(p, re)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
    // Variable-path opens (env fallback / caller-provided path) that reference
    // the natives.db literal elsewhere in the same file are flagged as
    // candidates (fail-closed; ledger may review false positives).
    const src = readFileSync(p, 'utf8');
    if (/natives\.db/.test(src) && !/NativesDbBroker|default_assistant_db_path/.test(src)) {
      const opens = nonCommentLines(p, /(Connection::open|open_with_flags)\(/);
      for (const h of opens) {
        const key = `${relToRoot(p)}:${h.line}`;
        if (!map.has(key)) map.set(key, `variable-path open near natives.db literal — ${h.text}`);
      }
    }
  }
  return map;
}

function collectCrossDbHost(root = ROOT) {
  const map = new Map();
  const hostRoot = join(root, 'src-tauri/src');
  if (!existsSync(hostRoot)) return map;
  for (const p of walk(hostRoot)) {
    if (!p.endsWith('.rs')) continue;
    if (isTestFile(p)) continue;
    // Host must never open the Daemon-authoritative assistant.db. `get_assistant_db_conn`
    // is the pooled open; a direct `Connection::open` of the assistant.db literal is
    // equally a violation. Setting the NATIVES_ASSISTANT_DB_PATH env var only hands the
    // path to the daemon sidecar (which owns the file) and is not a Host open.
    const re = /get_assistant_db_conn|init_assistant_db|(Connection::open)\([^)]*assistant\.db/;
    for (const h of nonCommentLines(p, re)) {
      map.set(`${relToRoot(p)}:${h.line}`, h.text);
    }
    // Variable-path opens (caller-provided db path) referencing the assistant.db
    // literal in the same file are flagged as candidates (fail-closed). Opens
    // whose argument names reference natives.db (the Host's own store) are
    // excluded — they are the Host-authority store, not the Daemon's.
    const src = readFileSync(p, 'utf8');
    if (/assistant\.db/.test(src) && !/NATIVES_ASSISTANT_DB_PATH/.test(src)) {
      const opens = nonCommentLines(p, /(Connection::open|open_with_flags)\(/);
      for (const h of opens) {
        if (/\b(natives_path|natives_conn|natives_db_path)\b/.test(h.text)) continue;
        const key = `${relToRoot(p)}:${h.line}`;
        if (!map.has(key)) map.set(key, `variable-path open near assistant.db literal — ${h.text}`);
      }
    }
  }
  return map;
}

function collectEmbeddedProd(root = ROOT) {
  const map = new Map();
  for (const r of [join(root, 'src-tauri/src'), join(root, 'src-agent-daemon/src')]) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      for (const h of nonCommentLines(p, /EmbeddedAuthority/)) {
        map.set(`${relToRoot(p)}:${h.line}`, h.text);
      }
    }
  }
  return map;
}

function collectGlobalSingleton(root = ROOT) {
  const map = new Map();
  for (const r of [join(root, 'src-tauri/src'), join(root, 'src-agent-daemon/src'), join(root, 'crates')]) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      for (const h of nonCommentLines(p, /static\s+mut|OnceLock<|lazy_static!|static\s+Lazy</)) {
        map.set(`${relToRoot(p)}:${h.line}`, h.text);
      }
    }
  }
  return map;
}

function collectUnregisteredInterval(root = ROOT) {
  const map = new Map();
  for (const p of walk(join(root, 'src'))) {
    if (!isHandwrittenTs(p)) continue;
    const rel = relToRoot(p);
    for (const h of nonCommentLines(p, /setInterval\s*\(/)) {
      map.set(`${rel}:${h.line}`, h.text);
    }
  }
  return map;
}

function collectHooksReverse(root = ROOT) {
  const map = new Map();
  const hooksRoot = join(root, 'src/hooks');
  if (!existsSync(hooksRoot)) return map;
  for (const p of walk(hooksRoot)) {
    if (!isHandwrittenTs(p)) continue;
    const src = readFileSync(p, 'utf8');
    for (const spec of importsOf(p, src)) {
      if (spec.startsWith('@/components/')) {
        map.set(`${relToRoot(p)}:${spec}`, `hook imports component ${spec}`);
      }
    }
  }
  return map;
}

// W4/W1: non-semantic clickable elements. A <div>/<span>/<li> with onClick must
// carry role="button" (or a semantic alternative) AND keyboard activation
// (onKeyDown), or it is flagged. Real <button>/<a>/<input> are fine.
// W1: uses the TSX AST so MULTILINE attributes are detected (the old line-based
// regex missed onClick spread over several lines).
function collectA11yClickable(root = ROOT) {
  const map = new Map();
  const roots = [join(root, 'src/components'), join(root, 'src/app')];
  for (const r of roots) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!isHandwrittenTs(p)) continue;
      if (/\.test\.(ts|tsx)$/.test(p)) continue;
      const src = readFileSync(p, 'utf8');
      // Fast path: no onClick anywhere -> skip AST parse.
      if (!/onClick\s*=/.test(src)) continue;
      const hits = a11yAstHits(src, relToRoot(p));
      for (const h of hits) map.set(h.key, h.text);
    }
  }
  return map;
}

function a11yAstHits(src, rel) {
  const hits = [];
  let ts;
  try {
    ts = loadTypeScript();
  } catch {
    // No typescript available: fall back to the line-based heuristic so the
    // gate still flags single-line violations (best effort, not a pass).
    const lines = src.split('\n');
    lines.forEach((line, i) => {
      const t = line.trim();
      if (!/<(div|span|li|section|header|footer|p|ul)[^>]*onClick=/.test(t)) return;
      if (/aria-hidden|role=["'](presentation|dialog|alertdialog)["']/.test(t)) return;
      if (/role=["'](button|link|menuitem|tab|checkbox|switch)["']/.test(t)) return;
      if (/onKeyDown|onKeyUp|onKeyPress/.test(t)) return;
      if (/<button|<a |<input|<select|<textarea/.test(t)) return;
      hits.push({ key: `${rel}:${i + 1}`, text: t.slice(0, 110) });
    });
    return hits;
  }
  const sf = ts.createSourceFile('x.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const NON_SEMANTIC = new Set(['div', 'span', 'li', 'section', 'header', 'footer', 'p', 'ul']);
  function attrsOf(elem) {
    if (ts.isJsxElement(elem)) return elem.openingElement.attributes?.properties;
    if (ts.isJsxSelfClosingElement(elem)) return elem.attributes?.properties;
    return undefined;
  }
  function attr(elem, name) {
    return attrsOf(elem)?.find((a) => ts.isJsxAttribute(a) && a.name?.text === name);
  }
  function visit(node) {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = ts.isJsxElement(node)
        ? node.openingElement.tagName?.text
        : node.tagName?.text;
      if (tag && NON_SEMANTIC.has(tag) && attr(node, 'onClick')) {
        const hasSemanticRole = ['button', 'link', 'menuitem', 'tab', 'checkbox', 'switch'].some((r) => {
          const role = attr(node, 'role');
          return role && role.initializer && role.initializer.text === r;
        });
        const hasKeyboard =
          attr(node, 'onKeyDown') || attr(node, 'onKeyUp') || attr(node, 'onKeyPress');
        const hasAriaHidden = attr(node, 'aria-hidden');
        const hasDialogRole = ['presentation', 'dialog', 'alertdialog'].some((r) => {
          const role = attr(node, 'role');
          return role && role.initializer && role.initializer.text === r;
        });
        if (!(hasSemanticRole || hasKeyboard || hasAriaHidden || hasDialogRole)) {
          const pos = sf.getLineAndCharacterOfPosition(node.getStart(sf));
          hits.push({ key: `${rel}:${pos.line + 1}`, text: src.slice(node.getStart(sf), node.getEnd(sf)).slice(0, 110) });
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(sf);
  return hits;
}

// W2/webpack_module_shadow: a top-level runtime binding named `module` in a
// client TS/TSX SourceFile shadows Webpack's factory parameter `module` inside
// the same direct eval that Next React Refresh appends HMR runtime to — the
// injected footer reads `module.hot.data` and crashes on the facade object
// (no `.hot`). Only TOP-LEVEL bindings matter: HMR injection is appended at
// module scope, so function/class-local `module` cannot shadow it.
// Allowed: export aliases (`export { moduleApi as module }`), property names
// (`api.module.list()`, `{ module: moduleApi }`), type-only imports, strings,
// comments. Fatal when found (no baseline exemption).
function collectWebpackModuleShadow(root = ROOT) {
  const map = new Map();
  const srcRoot = join(root, 'src');
  if (!existsSync(srcRoot)) return map;
  for (const p of walk(srcRoot)) {
    if (!isHandwrittenTs(p)) continue;
    if (/\.test\.(ts|tsx)$/.test(p)) continue;
    const src = readFileSync(p, 'utf8');
    let ts;
    try {
      ts = loadTypeScript();
    } catch {
      // No typescript available: best-effort line scan (not a pass). Only
      // top-level statement starts are considered; alias exports are skipped.
      const lines = src.split('\n');
      lines.forEach((line, i) => {
        const t = line.trim();
        if (/(^|;|\{)\s*(const|let|var)\s+module\b/.test(t) && !/as\s+module\b/.test(t)) {
          map.set(`${relToRoot(p)}:${i + 1}`, t.slice(0, 110));
        }
      });
      continue;
    }
    const sf = ts.createSourceFile('x.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const isTypeOnlyStmt = (s) =>
      (s.modifiers || []).some((m) => m.kind === ts.SyntaxKind.DeclareKeyword) ||
      (s.modifiers || []).some((m) => m.kind === ts.SyntaxKind.ExportKeyword) && s.isTypeOnly;
    for (const stmt of sf.statements) {
      if (isTypeOnlyStmt(stmt)) continue;
      let binding = null;
      if (ts.isVariableStatement(stmt)) {
        for (const decl of stmt.declarationList.declarations) {
          const names = bindingNamesOf(ts, decl.name);
          if (names.includes('module')) {
            binding = decl.name.getText(sf);
            break;
          }
        }
      } else if (
        (ts.isFunctionDeclaration(stmt) || ts.isClassDeclaration(stmt)) &&
        stmt.name &&
        stmt.name.text === 'module'
      ) {
        binding = stmt.name.text;
      } else if (ts.isImportDeclaration(stmt)) {
        const clause = stmt.importClause;
        if (!clause || clause.isTypeOnly) continue;
        if (clause.name && clause.name.text === 'module') {
          binding = clause.name.text;
        } else if (clause.namedBindings && ts.isNamespaceImport(clause.namedBindings)) {
          if (clause.namedBindings.name.text === 'module') binding = clause.namedBindings.name.text;
        } else if (clause.namedBindings && ts.isNamedImports(clause.namedBindings)) {
          for (const spec of clause.namedBindings.elements) {
            if (spec.isTypeOnly) continue;
            const local = spec.name.text;
            if (local === 'module') {
              binding = local;
              break;
            }
          }
        }
      }
      if (binding !== null) {
        const pos = sf.getLineAndCharacterOfPosition(stmt.getStart(sf));
        map.set(`${relToRoot(p)}:${pos.line + 1}`, `top-level runtime binding \`${binding}\` shadows webpack module`);
      }
    }
  }
  return map;
}

// Collect the local binding names of a variable declaration name node,
// including destructuring patterns (`const { module } = obj` binds `module`;
// `const { module: x } = obj` binds `x` and is safe).
function bindingNamesOf(ts, nameNode) {
  const out = [];
  const visit = (n) => {
    if (ts.isIdentifier(n)) {
      out.push(n.text);
    } else if (ts.isObjectBindingPattern(n) || ts.isArrayBindingPattern(n)) {
      for (const el of n.elements) {
        if (ts.isBindingElement(el)) visit(el.name);
      }
    }
  };
  visit(nameNode);
  return out;
}

function collectBudgetFunctions(root = ROOT) {
  const map = new Map();
  for (const r of [join(root, 'src-tauri/src'), join(root, 'src-agent-daemon/src'), join(root, 'crates')]) {
    if (!existsSync(r)) continue;
    for (const p of walk(r)) {
      if (!p.endsWith('.rs')) continue;
      if (isTestFile(p)) continue;
      const lines = readFileSync(p, 'utf8').split('\n');
      let fnStart = -1;
      let brace = 0;
      let started = false;
      for (let i = 0; i < lines.length; i++) {
        const t = lines[i].trim();
        if (/^(pub\s+)?(pub\([^)]*\)\s+)?(async\s+)?(unsafe\s+)?fn\s+\w+/.test(t)) {
          if (fnStart >= 0 && started && i - fnStart > 120) {
            map.set(`${relToRoot(p)}:${fnStart + 1}`, `fn body ~${i - fnStart} lines`);
          }
          fnStart = i;
          started = false;
          brace = 0;
          const open = (t.match(/{/g) || []).length;
          const close = (t.match(/}/g) || []).length;
          brace = open - close;
          if (brace > 0) started = true;
          continue;
        }
        if (fnStart >= 0 && started) {
          brace += (t.match(/{/g) || []).length - (t.match(/}/g) || []).length;
          if (brace <= 0) {
            if (i - fnStart > 120) map.set(`${relToRoot(p)}:${fnStart + 1}`, `fn body ~${i - fnStart} lines`);
            fnStart = -1;
            started = false;
          }
        }
      }
    }
  }
  return map;
}

// ---------------------------------------------------------------------------
// Checks registry: id -> { collect, fail: bool }
// ---------------------------------------------------------------------------

const CHECKS = [
  { id: 'over_1000', collect: collectOver1000, fail: true },
  { id: 'over_700_ledger', collect: collectOver700, fail: false },
  { id: 'ui_business', collect: collectUiBusiness, fail: true },
  { id: 'feature_cross', collect: collectFeatureCross, fail: true },
  { id: 'thick_page', collect: collectThickPages, fail: true },
  { id: 'raw_invoke', collect: collectRawInvoke, fail: true },
  { id: 'native_dialog', collect: collectNativeDialog, fail: true },
  { id: 'cross_db_daemon', collect: collectCrossDbDaemon, fail: true },
  { id: 'cross_db_host', collect: collectCrossDbHost, fail: true },
  { id: 'embedded_prod', collect: collectEmbeddedProd, fail: true },
  { id: 'global_singleton', collect: collectGlobalSingleton, fail: true },
  { id: 'unregistered_interval', collect: collectUnregisteredInterval, fail: false },
  { id: 'hooks_reverse', collect: collectHooksReverse, fail: true },
  { id: 'a11y_clickable', collect: collectA11yClickable, fail: true },
  { id: 'webpack_module_shadow', collect: collectWebpackModuleShadow, fail: true },
  { id: 'budget_functions', collect: collectBudgetFunctions, fail: false },
];

function loadManifest() {
  if (!existsSync(MANIFEST)) return {};
  try {
    return JSON.parse(readFileSync(MANIFEST, 'utf8'));
  } catch {
    return {};
  }
}

// W1 fail-closed helpers (exported for the mutation fixtures):
//  - fatal checks (fail:true) are NEVER silenced by the manifest: any found
//    entry makes the gate fail, regardless of whether it is a known debt.
//  - an empty scan scope (zero scanned files) is fatal: it must not be
//    indistinguishable from "everything clean".
function isFatalCheck(id) {
  const c = CHECKS.find((x) => x.id === id);
  return Boolean(c && c.fail);
}

function shouldFailCheck(check, found, known) {
  if (!check.fail) return false;
  return found.size > 0;
}

function shouldFailEmptyScope(summary) {
  return summary.scannedFiles === 0;
}

function runChecks(root = ROOT) {
  const manifest = loadManifest();
  const rows = [];
  let scannedFiles = 0;
  let newCount = 0;
  let knownCount = 0;
  const violations = [];
  for (const check of CHECKS) {
    const found = check.collect(root);
    const known = manifest[check.id] || {};
    const knownKeys = new Set(Object.keys(known));
    let newEntries = 0;
    let knownEntries = 0;
    for (const [key, reason] of found) {
      if (knownKeys.has(key)) knownEntries += 1;
      else newEntries += 1;
    }
    scannedFiles += found.size > 0 ? 1 : 0;
    knownCount += knownEntries;
    newCount += newEntries;
    const fail = shouldFailCheck(check, found, known);
    rows.push({
      id: check.id,
      fail: check.fail,
      total: found.size,
      known: knownEntries,
      new: newEntries,
      status: fail ? 'FAIL' : check.fail ? 'ok' : 'ledger',
    });
    for (const [key, reason] of found) {
      violations.push({ check: check.id, severity: check.fail ? 'ERROR' : 'WARN', key, reason });
    }
  }
  return { rows, violations, newCount, knownCount, scannedFiles };
}

function main() {
  const rootArg = process.argv[2];
  const root = rootArg && !rootArg.startsWith('--') ? resolve(ROOT, rootArg) : ROOT;
  if (BASELINE) {
    const next = {};
    for (const check of CHECKS) {
      const found = check.collect(root);
      next[check.id] = Object.fromEntries(found);
    }
    writeFileSync(MANIFEST, `${JSON.stringify(next, null, 2)}\n`);
    console.log('architecture:check baseline written to scripts/architecture-debt-manifest.json');
    for (const k of Object.keys(next)) console.log(`  - ${k}: ${Object.keys(next[k]).length}`);
    process.exit(0);
  }

  const summary = runChecks(root);
  const emptyScope = shouldFailEmptyScope(summary);
  for (const v of summary.violations) {
    console.log(`[${v.severity}] ${v.check}: ${v.key} — ${v.reason}`);
  }
  console.log('---');
  for (const r of summary.rows) console.log(`  [${r.status}] ${r.id}: total=${r.total} known=${r.known} new=${r.new}`);
  const failing = summary.rows.filter((r) => r.status === 'FAIL');
  const failed = failing.length > 0 || emptyScope;
  console.log(
    `architecture:check ${failed ? 'FAILED' : 'OK'} — known(debt): ${summary.knownCount}, new: ${summary.newCount}, scanned-files: ${summary.scannedFiles}`,
  );
  // Machine-readable summary (JSON) on stdout for CI/evidence.
  process.stdout.write(`${JSON.stringify({ ok: !failed, emptyScope, fatal: failing.map((r) => r.id), newCount: summary.newCount, knownCount: summary.knownCount, scannedFiles: summary.scannedFiles }, null, 2)}\n`);
  if (failed) {
    if (emptyScope) console.error('Empty scan scope: nothing scanned, cannot claim clean.');
    process.exit(1);
  }
  process.exit(0);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}

export {
  collectOver1000,
  collectOver700,
  collectUiBusiness,
  collectFeatureCross,
  collectThickPages,
  collectRawInvoke,
  collectNativeDialog,
  collectCrossDbDaemon,
  collectCrossDbHost,
  collectEmbeddedProd,
  collectGlobalSingleton,
  collectUnregisteredInterval,
  collectHooksReverse,
  collectA11yClickable,
  collectWebpackModuleShadow,
  collectBudgetFunctions,
  isFatalCheck,
  shouldFailCheck,
  shouldFailEmptyScope,
  runChecks,
  CHECKS,
  loadManifest,
};
