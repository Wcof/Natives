import assert from 'node:assert/strict';
import { readdir, readFile, stat } from 'node:fs/promises';
import { resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));

const IGNORED_DIRS = new Set([
  'node_modules', 'target', 'dist', '.git', '.cargo', 'docs', '.runtime-evidence', 'third_party',
]);

const ALLOWED_EXCEED_REASONS = new Map([
  ['crates/native-file-host/src/tests.rs', 'Aggregated host test suite containing extensive IPC scenarios'],
  ['crates/file-manager-core/src/file_manager/file_manager_tests.rs', 'Comprehensive core filesystem operations test suite'],
]);

async function scanFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const results = [];
  for (const entry of entries) {
    if (IGNORED_DIRS.has(entry.name)) continue;
    const fullPath = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...await scanFiles(fullPath));
    } else if (/\.(js|mjs|rs|ts|tsx|go)$/.test(entry.name)) {
      results.push(fullPath);
    }
  }
  return results;
}

const allFiles = await scanFiles(ROOT);
const violations = [];
const reviews = [];
const stats = [];

for (const filePath of allFiles) {
  const relPath = relative(ROOT, filePath);
  const content = await readFile(filePath, 'utf8');
  const lines = content.split('\n');
  const lineCount = lines.length;
  const isTest = relPath.includes('.test.') || relPath.endsWith('_test.go') || relPath.endsWith('tests.rs');

  stats.push({ file: relPath, lines: lineCount });

  // Rule 1: No file >= 1000 lines (production code)
  if (lineCount >= 1000 && !isTest) {
    violations.push(`[HARD FAILURE] ${relPath} has ${lineCount} lines (>= 1000 limit)`);
  }

  // Rule 2: Files >= 700 lines must have registered rationale
  if (lineCount >= 700) {
    if (!ALLOWED_EXCEED_REASONS.has(relPath)) {
      violations.push(`[UNREGISTERED >= 700] ${relPath} has ${lineCount} lines (missing review record)`);
    } else {
      reviews.push(`[REGISTERED REVIEW] ${relPath} (${lineCount} lines): ${ALLOWED_EXCEED_REASONS.get(relPath)}`);
    }
  }

  // Rule 3: No window.prompt / alert in extension production code
  if (relPath.startsWith('extension/') && !isTest && !relPath.endsWith('.test.mjs')) {
    if (/\bwindow\.(prompt|alert|confirm)\b/.test(content)) {
      violations.push(`[FORBIDDEN DIALOG API] ${relPath} contains synchronous window.prompt/alert/confirm`);
    }
  }
}

console.log('--- Architecture Scale Audit ---');
stats.sort((a, b) => b.lines - a.lines);
console.log('Top 10 largest files:');
for (const item of stats.slice(0, 10)) {
  console.log(`  ${String(item.lines).padStart(5)} lines : ${item.file}`);
}

if (reviews.length) {
  console.log('\nRegistered >=700 lines:');
  reviews.forEach((r) => console.log('  ' + r));
}

if (violations.length) {
  console.error('\nArchitecture violations found:');
  violations.forEach((v) => console.error('  ' + v));
  process.exit(1);
} else {
  console.log('\nArchitecture check passed: zero >=1000 lines, forbidden APIs clean, all >=700 accounted for.');
}
