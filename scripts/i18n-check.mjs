#!/usr/bin/env node
/* global console, process */
/**
 * i18n checker (v3)
 *
 * Verifies two independent i18n gates:
 *
 *  1. Key sync (R-I3): zh.ts and en.ts must expose identical key sets.
 *  2. Bypass detection (R-I1): production TS/TSX must not contain
 *     user-visible copy that sidesteps the dictionary:
 *       (a) CJK string literals / JSX text outside the dictionaries, tests,
 *           and a small documented allowlist; and
 *       (b) locale-conditional ternaries that yield string literals
 *           (e.g. `zh ? '...' : '...'`, `locale === 'zh' ? ... : ...`),
 *           except BCP-47 locale tags used for date/number formatting.
 *
 * Usage: node scripts/i18n-check.mjs
 */

import { readFileSync, readdirSync, statSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, resolve, join, relative } from 'path';
import * as ts from 'typescript';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const SRC = resolve(ROOT, 'src');

// ────────────────────────────────────────────────────────────────────────
// 1. Key sync (existing behavior)
// ────────────────────────────────────────────────────────────────────────

function extractKeys(content) {
  const keys = new Set();
  let objStr = content.replace(/^export\s+const\s+\w+\s*=\s*/, '').trim();
  objStr = objStr.replace(/;\s*$/, '');
  if (!objStr.startsWith('{') || !objStr.endsWith('}')) {
    console.error('Cannot parse locale file - not a valid object literal?');
    return keys;
  }
  let i = 0;
  const chars = [...objStr];
  const len = chars.length;
  if (chars[i] === '{') i++;
  const pathStack = [];
  while (i < len) {
    const ch = chars[i];
    if (ch === ' ' || ch === '\n' || ch === '\t' || ch === '\r' || ch === ',') {
      i++;
      continue;
    }
    if (ch === '/' && chars[i + 1] === '/') {
      while (i < len && chars[i] !== '\n') i++;
      continue;
    }
    if (ch === '/' && chars[i + 1] === '*') {
      i += 2;
      while (i < len - 1 && !(chars[i] === '*' && chars[i + 1] === '/')) i++;
      i += 2;
      continue;
    }
    if (ch === '}') {
      pathStack.pop();
      i++;
      continue;
    }
    if (/[a-zA-Z_]/.test(ch)) {
      let key = '';
      while (i < len && /[a-zA-Z0-9_]/.test(chars[i])) {
        key += chars[i];
        i++;
      }
      while (i < len && chars[i] === ' ') i++;
      if (i < len && chars[i] === ':') {
        i++;
        while (i < len && chars[i] === ' ') i++;
        if (i < len) {
          const nextCh = chars[i];
          if (nextCh === '{') {
            pathStack.push(key);
            i++;
          } else if (nextCh === "'" || nextCh === '"') {
            const fullPath = [...pathStack, key].join('.');
            keys.add(fullPath);
            const quote = nextCh;
            i++;
            while (i < len && chars[i] !== quote) {
              if (chars[i] === '\\') i++;
              i++;
            }
            if (i < len) i++;
          } else if (/[tfn\d-]/.test(nextCh)) {
            keys.add([...pathStack, key].join('.'));
            while (i < len && chars[i] !== ',' && chars[i] !== '}' && chars[i] !== '\n') i++;
          } else if (nextCh === '[') {
            keys.add([...pathStack, key].join('.'));
            while (i < len && chars[i] !== ',' && chars[i] !== '}') i++;
          } else if (nextCh === '/') {
            continue;
          }
        }
      }
      continue;
    }
    i++;
  }
  return keys;
}

const zhContent = readFileSync(resolve(ROOT, 'src/i18n/zh.ts'), 'utf8');
const enContent = readFileSync(resolve(ROOT, 'src/i18n/en.ts'), 'utf8');
const zhKeys = extractKeys(zhContent);
const enKeys = extractKeys(enContent);
const missingInEn = [...zhKeys].filter((k) => !enKeys.has(k));
const missingInZh = [...enKeys].filter((k) => !zhKeys.has(k));

let exitCode = 0;
if (missingInEn.length > 0) {
  console.error(`\n❌ Missing in en.ts (${missingInEn.length}):`);
  missingInEn.sort().forEach((k) => console.error(`  - ${k}`));
  exitCode = 1;
}
if (missingInZh.length > 0) {
  console.error(`\n❌ Missing in zh.ts (${missingInZh.length}):`);
  missingInZh.sort().forEach((k) => console.error(`  - ${k}`));
  exitCode = 1;
}
if (exitCode === 0) {
  console.log(`✅ i18n keys in sync: ${zhKeys.size} zh = ${enKeys.size} en`);
} else {
  console.error(`\nzh: ${zhKeys.size} keys, en: ${enKeys.size} keys`);
}

// ────────────────────────────────────────────────────────────────────────
// 2. Bypass detection (R-I1)
// ────────────────────────────────────────────────────────────────────────

const CJK_RE = /[㐀-鿿]/;

/**
 * Allowlisted files: structured data / model-facing content, not UI chrome.
 * - provider-presets.ts: bilingual provider registry (proper nouns + metadata,
 *   consumed data-driven via `nameZh`/`descriptionZh` fields).
 * - prompt-context-injector.ts: generator LLM prompt template (Chinese spec
 *   text sent to the model — not user-visible UI copy).
 */
const FILE_ALLOWLIST = new Set([
  resolve(SRC, 'lib/provider-presets.ts'),
  resolve(SRC, 'lib/prompt-context-injector.ts'),
]);

/** BCP-47 locale tags are data for Intl formatting, not copy. */
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
  if (ts.isIdentifier(node)) {
    return node.text === 'locale' || node.text === 'zh' || node.text === 'isZh';
  }
  if (ts.isParenthesizedExpression(node)) return isLocaleRef(node.expression);
  return false;
}

function isLocaleCondition(node) {
  if (!node) return false;
  if (isLocaleRef(node)) return true;
  if (ts.isCallExpression(node)) {
    const expr = node.expression;
    if (ts.isPropertyAccessExpression(expr)) {
      // locale.startsWith(...) / locale.includes(...) — only when the
      // receiver is a locale reference (NOT `line.includes(...)` etc).
      if (isLocaleRef(expr.expression)) return true;
    }
    if (ts.isIdentifier(expr) && expr.text === 'uiLocale') return true;
    return isLocaleCondition(expr);
  }
  if (ts.isBinaryExpression(node)) {
    return isLocaleCondition(node.left) || isLocaleCondition(node.right);
  }
  if (ts.isPropertyAccessExpression(node)) return isLocaleCondition(node.expression);
  if (ts.isParenthesizedExpression(node)) return isLocaleCondition(node.expression);
  return false;
}

/**
 * Is a string literal branch user-visible copy rather than a class name /
 * field identifier / format token? Copy heuristic:
 *  - contains CJK, or
 *  - contains a space with letters (multi-word sentence), or
 *  - starts with an uppercase letter followed by lowercase (Capitalized word).
 */
function isCopyString(text) {
  if (!text || text === '') return false;
  if (CJK_RE.test(text)) return true;
  if (/\s/.test(text) && /[A-Za-z]/.test(text)) return true;
  if (/^[A-Z][a-z]/.test(text)) return true;
  return false;
}

function stringLiteralText(node) {
  if (ts.isStringLiteral(node)) return node.text;
  if (ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  return null;
}

function isFormatToken(text) {
  return FORMAT_TOKEN_ALLOWLIST.has(text);
}

function checkFile(file, violations) {
  const rel = relative(ROOT, file);
  const source = readFileSync(file, 'utf8');
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS);

  function visit(node) {
    // Rule (a): CJK string literals / template literals.
    const litText = stringLiteralText(node);
    if (litText !== null && CJK_RE.test(litText)) {
      const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
      violations.push(`CJK literal (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(litText)}`);
    }

    // Rule (b): locale-conditional expressions yielding string literals.
    if (ts.isConditionalExpression(node)) {
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
    // `zh && '...'` / `zh || '...'` short-circuit copy.
    if (ts.isBinaryExpression(node) && (node.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken || node.operatorToken.kind === ts.SyntaxKind.BarBarToken)) {
      if (isLocaleCondition(node.left)) {
        const text = stringLiteralText(node.right);
        if (text !== null && !isFormatToken(text) && isCopyString(text)) {
          const { line } = sf.getLineAndCharacterOfPosition(node.right.getStart(sf));
          violations.push(`locale short-circuit (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(text)}`);
        }
      }
    }

    // Rule (a) also covers CJK JSX text nodes.
    if (ts.isJsxText(node) && CJK_RE.test(node.text.trim())) {
      const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
      violations.push(`CJK JSX text (bypasses t()): ${rel}:${line + 1}  ${JSON.stringify(node.text.trim().slice(0, 60))}`);
    }

    ts.forEachChild(node, visit);
  }
  visit(sf);
}

const prodFiles = [];
walkFiles(SRC, prodFiles);
const violations = [];
for (const file of prodFiles) {
  if (FILE_ALLOWLIST.has(file)) continue;
  checkFile(file, violations);
}

if (violations.length > 0) {
  exitCode = 1;
  console.error(`\n❌ i18n bypass violations (${violations.length}):`);
  for (const v of violations) console.error(`  - ${v}`);
} else {
  console.log(`✅ i18n bypass scan clean: ${prodFiles.length} production files scanned`);
}

process.exit(exitCode);
