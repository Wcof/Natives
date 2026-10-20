import assert from 'node:assert/strict';
import { readdir, readFile, stat } from 'node:fs/promises';
import { resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));

const IGNORED_DIRS = new Set([
  'node_modules', 'target', 'dist', '.tmp', '.git', '.cargo', 'docs', '.runtime-evidence', 'third_party',
]);

const ALLOWED_EXCEED_REASONS = new Map([
  ['crates/native-file-host/src/tests.rs', 'Aggregated host test suite containing extensive IPC scenarios'],
  ['crates/file-manager-core/src/file_manager/file_manager_tests.rs', 'Comprehensive core filesystem operations test suite'],
  ['crates/app-runtime/src/main.rs', 'Natives product-level unified Built-in App Runtime shell and loopback HTTP server'],
  ['modules/fund/src/ledger.rs', 'Fund double-entry transaction and FIFO lot accounting engine with comprehensive unit tests'],
  ['modules/fund/src/portfolio.rs', 'Fund holding projection, cost calculation, and NAV replay with comprehensive unit tests'],
  ['modules/fund/src/api/home_api.rs', 'Fund home/finance/group-plan REST endpoints: single routing owner for the home surface; group-plan endpoints added by home remediation plan 11 with focused validation helpers and API tests; domain logic lives in group_plan.rs/goals.rs/finance.rs; analysis-report endpoints split to reports_api.rs.'],
  ['modules/fund/src/group_plan.rs', 'Custom money-group plan domain (plan 11 §4): plan CRUD/validation/transaction, same-snapshot group allocation projection, legacy import preview, and conservation fixture tests cohere as one domain module; projection and persistence share the same types.'],
  ['extension/files.js', 'Workspace shell controller: files.html surface, section routing and app-menu wiring'],
  ['extension/model-settings.js', 'Model settings dialog central controller: snapshot loop, action dispatch, OAuth/auth-file/quota flows over extracted per-domain controllers (usage/agent/advanced/oauth modules)'],
  ['scripts/apps/fixtures/sample-host/sample_host.rs', 'Built-in module Host fixture: activation verification, lock hierarchy, and runtime HTTP server'],
  ['modules/fund/src/otc_ops.rs', 'OTC fund order state machine (N09): create/confirm/cancel/arrival for buy, sell and two-leg conversions with cash reservation and trade_cash settlement couplings kept atomic in one domain module; plan scheduling lives in otc_plans.rs'],
  ['modules/fund/src/otc_ops_tests.rs', 'OTC fund operation integration tests (N09): holding derivation/calibration, order state machine with cash couplings, conversion legs and plan period dedup co-located as one scenario suite'],
  ['extension/tests/space-seven-widgets-verify.test.mjs', 'AI seven-widget space visual acceptance harness: appearance/font/chart-switch scenarios with Shadow DOM assertions'],
  ['modules/fund/ui/src/home.js', 'Fund home controller (plan 11/panwatch upgrade): tri-state display, group allocation projection, auto-information list/detail with personal state, goal/privacy/lifecycle wiring; evidence AI panel and manage dialogs live in home-evidence.js/home-manage.js'],
  ['modules/fund/ui/src/stock-detail.js', 'Stock detail workspace controller: data pipeline, session-aware polling lifecycle, detail panel and UI event binding; chart rendering already split into stock-detail-chart.js'],
  ['modules/fund/ui/src/positions-view.js', 'Fund positions controller: tab/account projection, holding rows with metrics band, add/transfer/edit dialogs and per-row action dispatch; N09 OTC operations delegated to otc-ops.js'],
  ['modules/fund/ui/src/stock-drawings.js', 'Stock drawings engine interaction layer: CRUD, magnet snap, tool/toolbar/popover event binding; pure geometry and graphic building already split into stock-drawings-shapes.js'],
  ['modules/fund/src/trends/chanlun_daily.rs', 'Single chan-theory daily algorithm pipeline (merged kline -> fractal -> stroke -> zhongshu -> divergence -> signal -> chart overlay); private structs span every stage so extraction would expose internal types'],
  ['crates/app-runtime-core/src/activation.rs', 'Product activation receipt v3 fail-closed verification state machine plus generation checks: one security invariant kept audit-complete in a single module'],
  ['modules/fund/src/market/handlers.rs', 'Market read-only HTTP handler set: independent thin handlers over one upstream quote client with shared cache conventions in a single route domain'],
  ['modules/fund/src/finance.rs', 'Fund finance core: cash position aggregation, cash events with atomic balance updates, reservations/settlement, and value snapshots; goals split out to goals.rs (07 R10 review)'],
  ['modules/fund/src/rules.rs', 'Watch rule engine: parameter validation, event state machine, run_check evaluation across rule types with quote/kline feeds; simulation split out to simulate.rs (07 R10 review)'],
  ['modules/fund/src/migration.rs', 'Fund schema v1-v7 migration ladder with backup/restore and step verification; v8-v15 late ladder split to migration_late.rs (07 R10 review)'],
  ['modules/fund/src/migration_late.rs', 'Fund schema v8-v15 migration ladder (information items, shared instrument ledger, offline assets, alerts, correction provenance, exit history, report soft-delete, thesis version): historical SQL constants kept verbatim from the accepted migration sequence.'],
  ['modules/fund/src/corrections.rs', 'Fund transaction corrections domain (plan 04/audit P0-3): correction preview/submit with per-field idempotency, soft delete/restore with cash-state protection and same-transaction position projection replay, plus comprehensive unit tests.'],
  ['modules/fund/src/offline.rs', 'Offline investment assets domain (plan 03/audit P0-2): four asset-class CRUD with fixed-point amounts, valuation history, partial/full exits with idempotent retry history, receivables, and co-located unit tests.'],
  ['modules/fund/src/home.rs', 'Fund home aggregation and AI evidence pack builder (plan 11/05): wealth snapshot, scope resolution with single-asset evidence constraining (E1/E2/E3/I), news impact context, and co-located evidence tests.'],
  ['modules/fund/src/assets.rs', 'Recorded investment assets (P1): manual-asset input validation/instrument keys, fund NAV currency valuation, and the single aggregation quality state machine shared by detail API and cards, with co-located unit tests'],
  ['extension/app.js', 'Generic built-in module owner page: Core product projection handshake, capability leases, embedded app session surface and app-menu wiring in one controller'],
  ['model-host/internal/usage/store.go', 'AI usage persistence store: SQLite schema/migrations, per-subject aggregation windows, pricing and rollout state guarded by one mutex-protected writer'],
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
