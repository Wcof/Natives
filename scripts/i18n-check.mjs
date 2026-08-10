#!/usr/bin/env node
/**
 * Natives i18n gate — W1 fail-closed.
 *
 * Key sync (zh.ts vs en.ts) is parsed with the TypeScript AST, spreading
 * composition roots (`export const zh = { ...app, ...nav }`) into their
 * per-domain files. Fail-closed conditions (any one FAILs the gate):
 *   - locale file cannot be parsed (syntax error)
 *   - locale file resolves to ZERO keys (empty object / failed spread)
 *   - a key exists in zh but not en, or en but not zh
 *   - a duplicate key is defined inside one locale object (last-write-wins
 *     would silently drop copy — a defect, not an accident)
 * Bypass detection (R-I1): CJK literals, locale ternaries, JSX text.
 *
 * Usage:
 *   node scripts/i18n-check.mjs
 */
import { readFileSync, readdirSync, statSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, resolve, join, relative } from 'path';
import { createRequire } from 'module';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const SRC = resolve(ROOT, 'src');

// typescript resolved lazily (worktree fallback to the main workspace).
const requireLocal = createRequire(import.meta.url);
let tsModule = null;
function loadTypeScript() {
  if (tsModule) return tsModule;
  const candidates = ['typescript', join(ROOT, 'node_modules', 'typescript')];
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
  throw new Error('typescript package unavailable (needed for i18n AST)');
}

// ────────────────────────────────────────────────────────────────────────
// 1. AST-based key extraction (fail-closed)
// ────────────────────────────────────────────────────────────────────────

/**
 * Extract dotted keys from a locale object literal via TS AST.
 * Returns { keys: Set, duplicates: string[], spreadRefs: string[], error: string|null }
 * `error` is set when the file is not a single export const object literal.
 */
export function extractObjectKeys(content) {
  const ts = loadTypeScript();
  const sf = ts.createSourceFile('locale.ts', content, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  // Fail-closed: any syntax diagnostic invalidates the whole file, even if the
  // AST still recovers (e.g. `{ a: ; }` parses to a PropertyAssignment).
  if (sf.parseDiagnostics && sf.parseDiagnostics.length > 0) {
    const msg = sf.parseDiagnostics.map((d) => (typeof d.messageText === 'string' ? d.messageText : 'parse error')).join('; ');
    return { keys: new Set(), duplicates: [], spreadRefs: [], error: `syntax: ${msg}` };
  }
  const keys = new Set();
  const duplicates = [];
  const spreadRefs = [];
  let rootObject = null;

  function walkObject(node, prefix) {
    for (const prop of node.properties) {
      if (ts.isSpreadAssignment(prop)) {
        if (ts.isIdentifier(prop.expression)) spreadRefs.push(prop.expression.text);
        continue;
      }
      if (!ts.isPropertyAssignment(prop) && !ts.isShorthandPropertyAssignment(prop)) continue;
      const name = prop.name;
      let keyName = null;
      if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
        keyName = name.text;
      }
      if (keyName === null) continue;
      const full = prefix ? `${prefix}.${keyName}` : keyName;
      const initializer = ts.isPropertyAssignment(prop) ? prop.initializer : null;
      if (initializer && ts.isObjectLiteralExpression(initializer)) {
        walkObject(initializer, full);
      } else {
        if (keys.has(full)) duplicates.push(full);
        keys.add(full);
      }
    }
  }

  const first = sf.statements.find((s) => ts.isVariableStatement(s));
  if (first && ts.isVariableStatement(first)) {
    for (const decl of first.declarationList.declarations) {
      if (decl.initializer && ts.isObjectLiteralExpression(decl.initializer)) {
        rootObject = decl.initializer;
        break;
      }
    }
  }
  if (!rootObject) {
    return { keys: new Set(), duplicates, spreadRefs, error: 'no export const object literal found' };
  }
  walkObject(rootObject, '');
  return { keys, duplicates, spreadRefs, error: null };
}

/**
 * Resolve a locale file's full key set, expanding `...domain` spread refs from
 * ./zh/<domain>.ts. Fail-closed: a missing domain file is an error (empty
 * spread would silently drop copy).
 */
export function resolveComposedKeys(localeDir, content, root = ROOT) {
  const parsed = extractObjectKeys(content);
  const keys = new Set(parsed.keys);
  const errors = [];
  if (parsed.error) errors.push(`parse: ${parsed.error}`);
  for (const ref of parsed.spreadRefs) {
    const domainPath = resolve(root, `src/i18n/${localeDir}/${ref}.ts`);
    let domainContent;
    try {
      domainContent = readFileSync(domainPath, 'utf8');
    } catch {
      errors.push(`missing domain file: ${relative(root, domainPath)}`);
      continue;
    }
    const d = extractObjectKeys(domainContent);
    if (d.error) {
      errors.push(`domain parse ${ref}: ${d.error}`);
      continue;
    }
    for (const k of d.keys) keys.add(k);
    for (const dup of d.duplicates) keys.delete(dup); // duplicates are defects; surface via errors path below
  }
  return { keys, duplicates: parsed.duplicates, errors };
}

/**
 * Full gate. Returns { exitCode, zhKeys, enKeys, missingInEn, missingInZh,
 * violations, parseErrors, emptyFiles }.
 */
export function runI18nCheck(root = ROOT) {
  const src = join(root, 'src');
  const zhPath = join(src, 'i18n/zh.ts');
  const enPath = join(src, 'i18n/en.ts');
  const violations = [];
  const parseErrors = [];
  const emptyFiles = [];
  let exitCode = 0;

  function fatal(msg) {
    violations.push(msg);
  }

  let zhContent;
  let enContent;
  try {
    zhContent = readFileSync(zhPath, 'utf8');
  } catch {
    fatal(`cannot read zh locale: ${relative(root, zhPath)}`);
    zhContent = '';
  }
  try {
    enContent = readFileSync(enPath, 'utf8');
  } catch {
    fatal(`cannot read en locale: ${relative(root, enPath)}`);
    enContent = '';
  }

  const zhRes = zhContent ? resolveComposedKeys('zh', zhContent, root) : { keys: new Set(), duplicates: [], errors: ['missing file'] };
  const enRes = enContent ? resolveComposedKeys('en', enContent, root) : { keys: new Set(), duplicates: [], errors: ['missing file'] };

  for (const [label, res] of [['zh', zhRes], ['en', enRes]]) {
    for (const err of res.errors) fatal(`[${label}] ${err}`);
    for (const dup of res.duplicates) fatal(`[${label}] duplicate key: ${dup}`);
    if (res.keys.size === 0 && !res.errors.some((e) => e.startsWith('missing domain'))) {
      emptyFiles.push(label);
      fatal(`[${label}] locale resolved to ZERO keys (empty object or parse failure)`);
    }
  }

  const missingInEn = [...zhRes.keys].filter((k) => !enRes.keys.has(k));
  const missingInZh = [...enRes.keys].filter((k) => !zhRes.keys.has(k));
  for (const k of missingInEn) fatal(`missing in en: ${k}`);
  for (const k of missingInZh) fatal(`missing in zh: ${k}`);

  if (violations.length > 0) exitCode = 1;
  return { exitCode, zhKeys: zhRes.keys.size, enKeys: enRes.keys.size, missingInEn, missingInZh, violations, parseErrors, emptyFiles };
}

// ────────────────────────────────────────────────────────────────────────
// 2. Bypass detection (R-I1) — unchanged behavior, now also fail-closed
// ────────────────────────────────────────────────────────────────────────

const CJK_RE = /[㐀-鿿]/;

/** Allowlisted structured-data files (see original rationale). */
const FILE_ALLOWLIST = new Set([
  resolve(SRC, 'lib/provider-presets.ts'),
  resolve(SRC, 'lib/prompt-context-injector.ts'),
]);

const FORMAT_TOKEN_ALLOWLIST = new Set(['zh', 'en', 'zh-CN', 'en-US']);

function walkFiles(dir, out) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === 'node_modules' || entry === '__tests__' || entry === 'i18n') continue;
      walkFiles(full, out);
    } else if (
      /\.(ts|tsx)$/.test(entry) &&
      !/\.test\.(ts|tsx)$/.test(entry) &&
      !/\.d\.ts$/.test(entry)
    ) {
      out.push(full);
    }
  }
}

function isLocaleRef(node) {
  if (!node) return false;
  if (tsModule.isIdentifier(node)) {
    return node.text === 'locale' || node.text === 'zh' || node.text === 'isZh';
  }
  if (tsModule.isParenthesizedExpression(node)) return isLocaleRef(node.expression);
  return false;
}

function isLocaleCondition(node) {
  if (!node) return false;
  if (isLocaleRef(node)) return true;
  if (tsModule.isCallExpression(node)) {
    const expr = node.expression;
    if (tsModule.isPropertyAccessExpression(expr)) {
      if (isLocaleRef(expr.expression)) return true;
    }
    if (tsModule.isIdentifier(expr) && expr.text === 'uiLocale') return true;
    return isLocaleCondition(expr);
  }
  if (tsModule.isBinaryExpression(node)) {
    return isLocaleCondition(node.left) || isLocaleCondition(node.right);
  }
  if (tsModule.isPropertyAccessExpression(node)) return isLocaleCondition(node.expression);
  if (tsModule.isParenthesizedExpression(node)) return isLocaleCondition(node.expression);
  return false;
}

function isCopyString(text) {
  if (!text || text === '') return false;
  if (CJK_RE.test(text)) return true;
  if (/\s/.test(text) && /[A-Za-z]/.test(text)) return true;
  if (/^[A-Z][a-z]/.test(text)) return true;
  return false;
}

function stringLiteralText(node) {
  if (tsModule.isStringLiteral(node)) return node.text;
  if (tsModule.isNoSubstitutionTemplateLiteral(node)) return node.text;
  return null;
}

function isFormatToken(text) {
  return FORMAT_TOKEN_ALLOWLIST.has(text);
}

function checkFile(file, violations) {
  const rel = relative(ROOT, file);
  const source = readFileSync(file, 'utf8');
  const sf = tsModule.createSourceFile(file, source, tsModule.ScriptTarget.Latest, true, file.endsWith('.tsx') ? tsModule.ScriptKind.TSX : tsModule.ScriptKind.TS);

  function visit(node) {
    const litText = stringLiteralText(node);
    if (litText !== null && CJK_RE.test(litText)) {
      const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
      violations.push(`CJK literal (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(litText)}`);
    }

    if (tsModule.isConditionalExpression(node)) {
      if (isLocaleCondition(node.condition)) {
        for (const branch of [node.whenTrue, node.whenFalse]) {
          const text = stringLiteralText(branch);
          if (text !== null && !isFormatToken(text) && isCopyString(text)) {
            const { line } = sf.getLineAndCharacterOfPosition(branch.getStart(sf));
            violations.push(`locale ternary (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(text)}`);
          }
        }
      }
    }
    if (tsModule.isBinaryExpression(node) && (node.operatorToken.kind === tsModule.SyntaxKind.AmpersandAmpersandToken || node.operatorToken.kind === tsModule.SyntaxKind.BarBarToken)) {
      if (isLocaleCondition(node.left)) {
        const text = stringLiteralText(node.right);
        if (text !== null && !isFormatToken(text) && isCopyString(text)) {
          const { line } = sf.getLineAndCharacterOfPosition(node.right.getStart(sf));
          violations.push(`locale short-circuit (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(text)}`);
        }
      }
    }

    if (tsModule.isJsxText(node) && CJK_RE.test(node.text.trim())) {
      const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
      violations.push(`CJK JSX text (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(node.text.trim().slice(0, 60))}`);
    }

    tsModule.forEachChild(node, visit);
  }
  visit(sf);
}

function runBypassScan(root = ROOT) {
  const src = join(root, 'src');
  const violations = [];
  const prodFiles = [];
  walkFiles(src, prodFiles);
  for (const file of prodFiles) {
    if (FILE_ALLOWLIST.has(file)) continue;
    checkFile(file, violations);
  }
  return { prodFiles: prodFiles.length, violations };
}

export function main() {
  loadTypeScript();
  let exitCode = 0;
  const gate = runI18nCheck(ROOT);
  exitCode = gate.exitCode;
  if (gate.missingInEn.length > 0) {
    console.error(`\n❌ Missing in en.ts (${gate.missingInEn.length}):`);
    gate.missingInEn.sort().forEach((k) => console.error(`  - ${k}`));
  }
  if (gate.missingInZh.length > 0) {
    console.error(`\n❌ Missing in zh.ts (${gate.missingInZh.length}):`);
    gate.missingInZh.sort().forEach((k) => console.error(`  - ${k}`));
  }
  if (exitCode === 0) {
    console.log(`✅ i18n keys in sync: ${gate.zhKeys} zh = ${gate.enKeys} en`);
  } else {
    for (const v of gate.violations) console.error(`❌ ${v}`);
    console.error(`\nzh: ${gate.zhKeys} keys, en: ${gate.enKeys} keys`);
  }

  const bypass = runBypassScan(ROOT);
  if (bypass.violations.length > 0) {
    exitCode = 1;
    console.error(`\n❌ i18n bypass violations (${bypass.violations.length}):`);
    for (const v of bypass.violations) console.error(`  - ${v}`);
  } else {
    console.log(`✅ i18n bypass scan clean: ${bypass.prodFiles} production files scanned`);
  }
  process.exit(exitCode);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  main();
}
