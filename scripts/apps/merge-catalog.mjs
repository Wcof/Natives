import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { checkCatalog } from './check-app-manifest.mjs';
import { checkPackageBudget } from './check-package-budget.mjs';
import { signCatalog } from './catalog-signing.mjs';

const directory = resolve(process.argv[2] || 'dist/app-release');
const entries = readdirSync(directory).filter((name) => /^catalog-(?:darwin|linux|windows)-.*\.json$/.test(name))
  .map((name) => JSON.parse(readFileSync(join(directory, name))));
if (!entries.length) throw new Error('no release catalog fragments');
const app = { ...entries[0], packages: entries.flatMap((entry) => {
  if (entry.app_id !== entries[0].app_id || entry.version !== entries[0].version) throw new Error('release version mismatch');
  return entry.packages;
}) };
const catalog = { catalogVersion: 1, publishedAt: new Date().toISOString(), apps: [app] };
checkCatalog(catalog);
checkPackageBudget(catalog);
const path = join(directory, 'catalog-v1.json');
writeFileSync(path, JSON.stringify(catalog, null, 2) + '\n');
signCatalog(path, process.argv[3]);
console.log(JSON.stringify({ catalog: path, platforms: app.packages.length }));
