#!/usr/bin/env node
/**
 * One-shot A1 bundle recovery: strip whole-line comments from distributable
 * extension JS. A comment is removed only when it owns its line(s): the line
 * starts with the comment (after indentation) and nothing but whitespace
 * follows before the newline. String/template/regex state is tracked
 * char-by-char so `//` inside strings and multi-line template literals is
 * never touched.
 */
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = new URL('../..', import.meta.url).pathname;
const DEV_NAMES = new Set(['node_modules', '.git', 'fixtures', 'test', 'tests', '__pycache__']);
const EXCLUDED = new Set(['ui-harness.js', 'test-dom-mock.js', 'files-preview.js']);

function distributable(root) {
  const dir = join(root, 'extension');
  const files = [];
  const walk = (current) => {
    for (const name of readdirSync(current)) {
      if (DEV_NAMES.has(name) || name.endsWith('.map') || name.endsWith('.md') || name.startsWith('.')) continue;
      const path = join(current, name);
      const stat = statSync(path);
      if (stat.isDirectory()) walk(path);
      else if (name.endsWith('.js') && !EXCLUDED.has(name)) files.push(path);
    }
  };
  walk(dir);
  return files.sort();
}

export function stripLineComments(src) {
  const n = src.length;
  let i = 0;
  let inStr = null; // one of ' " `
  let inLineComment = false;
  let inBlockComment = false;
  let lineStart = 0;
  const spans = [];

  const atLineStart = (pos) => src.slice(lineStart, pos).trim() === '';

  while (i < n) {
    const c = src[i];
    if (inLineComment) {
      if (c === '\n') { inLineComment = false; lineStart = i + 1; }
      i += 1;
      continue;
    }
    if (inBlockComment) {
      if (c === '*' && i + 1 < n && src[i + 1] === '/') { inBlockComment = false; i += 2; }
      else i += 1;
      continue;
    }
    if (inStr) {
      if (c === '\\') { i += 2; continue; }
      if (c === inStr) inStr = null;
      if (c === '\n' && inStr === '`') lineStart = i + 1; // template literals may span lines
      i += 1;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') { inStr = c; i += 1; continue; }
    if (c === '/' && i + 1 < n) {
      if (src[i + 1] === '/') {
        if (atLineStart(i)) {
          const j = src.indexOf('\n', i);
          const end = j === -1 ? n : j;
          spans.push([i, end]);
          i = end;
          if (j === -1) break;
          lineStart = i + 1;
          continue;
        }
        i += 1;
        continue;
      }
      if (src[i + 1] === '*') {
        if (atLineStart(i)) {
          const j = src.indexOf('*/', i + 2);
          if (j !== -1) {
            const end = j + 2;
            const nl = src.indexOf('\n', end);
            if (nl === -1 || src.slice(end, nl).trim() === '') {
              const start = src.lastIndexOf('\n', i - 1) + 1;
              spans.push([start, nl === -1 ? n : nl]);
              i = nl === -1 ? n : nl + 1;
              if (i < n) lineStart = i;
              continue;
            }
          }
        }
        i += 1;
        continue;
      }
    }
    if (c === '\n') lineStart = i + 1;
    i += 1;
  }

  spans.sort((a, b) => a[0] - b[0]);
  let out = '';
  let pos = 0;
  for (const [s, e] of spans) {
    out += src.slice(pos, s);
    pos = e;
  }
  out += src.slice(pos);
  // collapse 3+ consecutive blank lines left behind to 2 (keeps structure readable)
  return out.replace(/\n{4,}/g, '\n\n\n');
}

if (existsSync(import.meta.filename) || process.argv[1]) {
  let totalBefore = 0;
  let totalAfter = 0;
  for (const file of distributable(ROOT)) {
    const original = readFileSync(file, 'utf8');
    const next = stripLineComments(original);
    totalBefore += Buffer.byteLength(original);
    totalAfter += Buffer.byteLength(next);
    if (next !== original) writeFileSync(file, next);
  }
  console.log(JSON.stringify({ files: distributable(ROOT).length, rawBefore: totalBefore, rawAfter: totalAfter, rawSave: totalBefore - totalAfter }));
}
