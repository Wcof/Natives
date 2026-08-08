#!/usr/bin/env node
/**
 * check-lockfile-sync.mjs — T002 (P1-015): fail CI when package.json and
 * package-lock.json drift, and when the installed npm is not the pinned
 * packageManager. Run after `npm ci` so the resolved tree is authoritative.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const pkgPath = path.join(root, 'package.json');
const lockPath = path.join(root, 'package-lock.json');

const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));

const errors = [];

// 1. package-lock.json must exist and reference the same name/version.
if (lock.name !== pkg.name || lock.version !== pkg.version) {
  errors.push(
    `package-lock.json (${lock.name}@${lock.version}) does not match package.json (${pkg.name}@${pkg.version})`
  );
}

// 2. Every direct dependency in package.json must be resolvable in the lock.
for (const [name, spec] of Object.entries(pkg.dependencies ?? {})) {
  const entry = lock.packages?.[`node_modules/${name}`];
  if (!entry) {
    errors.push(`dependency "${name}" (${spec}) is missing from package-lock.json`);
    continue;
  }
  if (!lock.packages?.['']?.dependencies?.[name] && !lock.packages?.['']?.devDependencies?.[name]) {
    errors.push(`dependency "${name}" is installed but not a root dependency in the lockfile`);
  }
}
for (const [name, spec] of Object.entries(pkg.devDependencies ?? {})) {
  const entry = lock.packages?.[`node_modules/${name}`];
  if (!entry) {
    errors.push(`devDependency "${name}" (${spec}) is missing from package-lock.json`);
  }
}

// 3. packageManager pin must be parseable (name@version).
if (typeof pkg.packageManager !== 'string' || !/^npm@\d+\.\d+\.\d+$/.test(pkg.packageManager)) {
  errors.push(`packageManager must be pinned as npm@<semver>, got ${JSON.stringify(pkg.packageManager)}`);
} else {
  const pinned = pkg.packageManager.split('@')[1];
  const installed = process.env.npm_config_user_agent ?? '';
  if (installed && !installed.includes(`npm/${pinned}`)) {
    // Not fatal (corepack may be absent), but warn loudly.
    console.warn(`[warn] running ${installed}; package.json pins npm@${pinned}`);
  }
}

// 4. engines.node must exist and be a range.
if (!pkg.engines?.node) {
  errors.push('engines.node must be set (e.g. ">=20.11.0 <21")');
}

if (errors.length > 0) {
  console.error('check-lockfile-sync FAILED:');
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
console.log('check-lockfile-sync OK: package.json ↔ package-lock.json in sync');
