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
 *   - a production t() call uses a statically enumerable key absent from the locale
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

  if (zhRes.keys.size > 0) {
    const callsites = auditTranslationCallsites(root, zhRes.keys);
    for (const err of callsites.parseErrors) {
      parseErrors.push(err);
      fatal(`callsite parse: ${err}`);
    }
    for (const violation of callsites.violations) fatal(`missing callsite key: ${violation}`);
  }

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
      !/\.(test|spec)\.(ts|tsx)$/.test(entry) &&
      !/\.d\.ts$/.test(entry)
    ) {
      out.push(full);
    }
  }
}

function i18nImportBinding(node) {
  if (!tsModule.isImportDeclaration(node) || !tsModule.isStringLiteral(node.moduleSpecifier)) return null;
  const moduleName = node.moduleSpecifier.text;
  if (moduleName !== '@/i18n') return null;
  const bindings = node.importClause?.namedBindings;
  if (!bindings || !tsModule.isNamedImports(bindings)) return null;
  for (const element of bindings.elements) {
    const imported = element.propertyName?.text ?? element.name.text;
    if (imported === 't') return element.name;
  }
  return null;
}

function unwrapTranslationWrapper(node) {
  if (tsModule.isArrowFunction(node) || tsModule.isFunctionExpression(node)) return node;
  if (
    tsModule.isCallExpression(node) &&
    tsModule.isIdentifier(node.expression) &&
    node.expression.text === 'useCallback' &&
    node.arguments.length > 0
  ) {
    const candidate = node.arguments[0];
    if (tsModule.isArrowFunction(candidate) || tsModule.isFunctionExpression(candidate)) return candidate;
  }
  return null;
}

function returnedCallExpression(fn) {
  if (tsModule.isCallExpression(fn.body)) return fn.body;
  if (!tsModule.isBlock(fn.body)) return null;
  const statement = fn.body.statements.find((item) => tsModule.isReturnStatement(item));
  return statement?.expression && tsModule.isCallExpression(statement.expression)
    ? statement.expression
    : null;
}

function collectTranslationFunctions(sf, checker) {
  const functions = new Map();
  for (const statement of sf.statements) {
    const localBinding = i18nImportBinding(statement);
    const symbol = localBinding ? checker.getSymbolAtLocation(localBinding) : null;
    if (symbol) functions.set(symbol, 1);
  }

  let changed = true;
  while (changed) {
    changed = false;
    function visit(node) {
      if (tsModule.isVariableDeclaration(node) && tsModule.isIdentifier(node.name) && node.initializer) {
        const wrapper = unwrapTranslationWrapper(node.initializer);
        const call = wrapper ? returnedCallExpression(wrapper) : null;
        if (call && tsModule.isIdentifier(call.expression)) {
          const sourceSymbol = checker.getSymbolAtLocation(call.expression);
          const sourceKeyIndex = sourceSymbol ? functions.get(sourceSymbol) : undefined;
          const keyArg = sourceKeyIndex === undefined ? undefined : call.arguments[sourceKeyIndex];
          if (keyArg && tsModule.isIdentifier(keyArg)) {
            const wrapperKeyIndex = wrapper.parameters.findIndex(
              (parameter) => tsModule.isIdentifier(parameter.name) && parameter.name.text === keyArg.text,
            );
            const wrapperSymbol = checker.getSymbolAtLocation(node.name);
            if (wrapperSymbol && wrapperKeyIndex >= 0 && functions.get(wrapperSymbol) !== wrapperKeyIndex) {
              functions.set(wrapperSymbol, wrapperKeyIndex);
              changed = true;
            }
          }
        }
      }
      tsModule.forEachChild(node, visit);
    }
    visit(sf);
  }
  return functions;
}

/**
 * Return every string value that can be supplied by a statically enumerable
 * key expression. Unknown expressions are intentionally ignored; callers may
 * still contain dynamic values, but any literal branch remains auditable.
 */
function staticKeyLiterals(node) {
  if (!node) return [];
  if (tsModule.isStringLiteral(node) || tsModule.isNoSubstitutionTemplateLiteral(node)) {
    return [{ text: node.text, node }];
  }
  if (tsModule.isParenthesizedExpression(node)) {
    return staticKeyLiterals(node.expression);
  }
  if (tsModule.isConditionalExpression(node)) {
    return [...staticKeyLiterals(node.whenTrue), ...staticKeyLiterals(node.whenFalse)];
  }
  if (tsModule.isBinaryExpression(node)) {
    const operator = node.operatorToken.kind;
    if (operator === tsModule.SyntaxKind.BarBarToken || operator === tsModule.SyntaxKind.AmpersandAmpersandToken) {
      return [...staticKeyLiterals(node.left), ...staticKeyLiterals(node.right)];
    }
  }
  return [];
}

/**
 * Verify referential integrity for statically knowable production t() keys.
 * Dynamic expressions are intentionally outside this gate because their value
 * cannot be proven from syntax alone; literal branches of a composite
 * expression are still checked individually.
 */
export function auditTranslationCallsites(root, localeKeys) {
  loadTypeScript();
  const src = join(root, 'src');
  const prodFiles = [];
  const violations = [];
  const parseErrors = [];
  walkFiles(src, prodFiles);
  const program = tsModule.createProgram({
    rootNames: prodFiles,
    options: {
      jsx: tsModule.JsxEmit.Preserve,
      noLib: true,
      noResolve: true,
      skipLibCheck: true,
      target: tsModule.ScriptTarget.Latest,
    },
  });
  const checker = program.getTypeChecker();

  for (const file of prodFiles) {
    const sf = program.getSourceFile(file);
    if (!sf) {
      parseErrors.push(`${relative(root, file)}:1: source file unavailable`);
      continue;
    }
    if (sf.parseDiagnostics?.length > 0) {
      for (const diagnostic of sf.parseDiagnostics) {
        const position = diagnostic.start ?? 0;
        const { line } = sf.getLineAndCharacterOfPosition(position);
        parseErrors.push(`${relative(root, file)}:${line + 1}: syntax error`);
      }
      continue;
    }

    const translationFunctions = collectTranslationFunctions(sf, checker);
    if (translationFunctions.size === 0) continue;
    function visit(node) {
      if (tsModule.isCallExpression(node) && tsModule.isIdentifier(node.expression)) {
        const symbol = checker.getSymbolAtLocation(node.expression);
        const keyIndex = symbol ? translationFunctions.get(symbol) : undefined;
        const keyNode = keyIndex === undefined ? undefined : node.arguments[keyIndex];
        for (const literal of staticKeyLiterals(keyNode)) {
          if (!localeKeys.has(literal.text)) {
            const { line } = sf.getLineAndCharacterOfPosition(literal.node.getStart(sf));
            violations.push(`${relative(root, file)}:${line + 1}:${literal.text}`);
          }
        }
      }
      tsModule.forEachChild(node, visit);
    }
    visit(sf);
  }
  return { prodFiles: prodFiles.length, violations, parseErrors };
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
