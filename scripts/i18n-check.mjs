#!/usr/bin/env node
/**
 * i18n key sync checker (v2 - robust parser)
 * Verifies that zh.ts and en.ts have identical key sets (R-I3 compliance).
 * Usage: node scripts/i18n-check.mjs
 */

import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, resolve } from 'path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

/**
 * Extract all translation keys from a locale TS file by AST-like parsing.
 * Handles `export const zh = { ... }` and `export const en = { ... }` objects.
 */
function extractKeys(content) {
  const keys = new Set();

  // Strip the `export const zh = ` or `export const en = ` prefix
  let objStr = content.replace(/^export\s+const\s+\w+\s*=\s*/, '').trim();
  // Remove trailing semicolons
  objStr = objStr.replace(/;\s*$/, '');

  if (!objStr.startsWith('{') || !objStr.endsWith('}')) {
    console.error('Cannot parse locale file - not a valid object literal?');
    return keys;
  }

  // Find the export name to parse comments
  const exportMatch = content.match(/export\s+const\s+(\w+)\s*=\s*/);

  // Simple recursive object key extractor using brace matching
  // We manually walk the string to extract key: value pairs
  let i = 0;
  const chars = [...objStr];
  const len = chars.length;

  // Skip initial `{`
  if (chars[i] === '{') i++;

  // State
  const pathStack = [];
  const collectPath = () => pathStack.join('.');

  while (i < len) {
    const ch = chars[i];

    // Skip whitespace and commas
    if (ch === ' ' || ch === '\n' || ch === '\t' || ch === '\r' || ch === ',') {
      i++;
      continue;
    }

    // Skip single-line comments
    if (ch === '/' && chars[i + 1] === '/') {
      while (i < len && chars[i] !== '\n') i++;
      continue;
    }

    // Skip multi-line comments
    if (ch === '/' && chars[i + 1] === '*') {
      i += 2;
      while (i < len - 1 && !(chars[i] === '*' && chars[i + 1] === '/')) i++;
      i += 2;
      continue;
    }

    // Close brace — pop from stack
    if (ch === '}') {
      pathStack.pop();
      i++;
      // After closing a brace, there may be a comma (consumed at top of loop)
      continue;
    }

    // Open brace — for object literals nested directly (no key before)
    // But this is already handled when we push to pathStack on `key: {`

    // Key: alphanumeric string followed by :
    if (/[a-zA-Z_]/.test(ch)) {
      let key = '';
      while (i < len && /[a-zA-Z0-9_]/.test(chars[i])) {
        key += chars[i];
        i++;
      }
      // Skip whitespace then expect ':'
      while (i < len && chars[i] === ' ') i++;
      if (i < len && chars[i] === ':') {
        i++;
        // Skip whitespace after ':'
        while (i < len && chars[i] === ' ') i++;

        if (i < len) {
          const nextCh = chars[i];
          if (nextCh === '{') {
            // Nested object — push key to pathStack
            pathStack.push(key);
            i++; // skip '{'
          } else if (nextCh === "'" || nextCh === '"') {
            // String value — this is a leaf key
            const fullPath = [...pathStack, key].join('.');
            keys.add(fullPath);
            // Skip to end of string
            const quote = nextCh;
            i++;
            while (i < len && chars[i] !== quote) {
              if (chars[i] === '\\') i++; // skip escaped char
              i++;
            }
            if (i < len) i++; // skip closing quote
          } else if (/[tfn\d\-]/.test(nextCh)) {
            // Boolean/number value — leaf key
            const fullPath = [...pathStack, key].join('.');
            keys.add(fullPath);
            while (i < len && chars[i] !== ',' && chars[i] !== '}' && chars[i] !== '\n') i++;
          } else if (nextCh === '[') {
            // Array value — leaf key
            const fullPath = [...pathStack, key].join('.');
            keys.add(fullPath);
            while (i < len && chars[i] !== ',' && chars[i] !== '}') i++;
          } else if (nextCh === '/') {
            // Comment starts — skip
            continue;
          }
        }
      }
      continue;
    }

    // Skip anything else
    i++;
  }

  return keys;
}

const zhContent = readFileSync(resolve(ROOT, 'src/i18n/zh.ts'), 'utf8');
const enContent = readFileSync(resolve(ROOT, 'src/i18n/en.ts'), 'utf8');

const zhKeys = extractKeys(zhContent);
const enKeys = extractKeys(enContent);

const missingInEn = [...zhKeys].filter(k => !enKeys.has(k));
const missingInZh = [...enKeys].filter(k => !zhKeys.has(k));

let exitCode = 0;

if (missingInEn.length > 0) {
  console.error(`\n❌ Missing in en.ts (${missingInEn.length}):`);
  missingInEn.sort().forEach(k => console.error(`  - ${k}`));
  exitCode = 1;
}

if (missingInZh.length > 0) {
  console.error(`\n❌ Missing in zh.ts (${missingInZh.length}):`);
  missingInZh.sort().forEach(k => console.error(`  - ${k}`));
  exitCode = 1;
}

if (exitCode === 0) {
  console.log(`\n✅ i18n keys in sync: ${zhKeys.size} zh = ${enKeys.size} en`);
} else {
  console.error(`\nzh: ${zhKeys.size} keys, en: ${enKeys.size} keys`);
}

process.exit(exitCode);
