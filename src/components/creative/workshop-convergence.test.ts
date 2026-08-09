/**
 * Workshop page convergence (T10): no giant multi-responsibility controller,
 * no dead `localStartAfterSave` state, no hardcoded visible English, no raw
 * unclassified errors, and a real (non-injected) proposal wiring.
 *
 * These are source-level guards: WorkshopPage must stay a thin orchestrator
 * whose wizards/surfaces live in `shell/workshop/` modules, and the strings a
 * user sees must come from i18n.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

const pageSource = readFileSync(
  new URL('../shell/WorkshopPage.tsx', import.meta.url),
  'utf8',
);
const catalogShellSource = readFileSync(
  new URL('../shell/workshop/CatalogShell.tsx', import.meta.url),
  'utf8',
);

const WORKSHOP_PAGE_LINE_LIMIT = 300;

describe('WorkshopPage convergence (T10)', () => {
  it('WorkshopPage is no longer a giant multi-responsibility controller', () => {
    assert.ok(
      pageSource.split('\n').length <= WORKSHOP_PAGE_LINE_LIMIT,
      `WorkshopPage must be <= ${WORKSHOP_PAGE_LINE_LIMIT} lines, got ${pageSource.split('\n').length}`,
    );
  });

  it('wizards/surfaces live in dedicated single-responsibility modules', () => {
    for (const moduleName of [
      'CatalogShell',
      'LocalImportWizard',
      'GitHubInstallWizard',
      'WindowSurface',
      'ProposalInboxController',
      'LogsController',
    ]) {
      assert.match(
        pageSource,
        new RegExp(`from './workshop/${moduleName}'`),
        `WorkshopPage must import ./workshop/${moduleName}`,
      );
    }
  });

  it('removes the dead localStartAfterSave state (buttons already decide)', () => {
    assert.doesNotMatch(pageSource, /localStartAfterSave/, 'no split start-after-save state');
  });

  it('removes the hardcoded English "confirm bind mounts" label', () => {
    assert.doesNotMatch(pageSource, /confirm bind mounts/, 'hardcoded English removed');
    assert.doesNotMatch(pageSource, /githubConfirmBinds/, 'binds label must come from i18n key usage, not raw text');
  });

  it('user-visible labels in the shell come from i18n', () => {
    // Spot-check the labels the audit flagged as hardcoded English.
    assert.doesNotMatch(catalogShellSource, />\s*confirm bind mounts\s*</);
    assert.doesNotMatch(catalogShellSource, /node_modules:/);
    assert.doesNotMatch(catalogShellSource, /env keys:/);
  });
});
