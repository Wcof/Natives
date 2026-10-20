import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const css = readFileSync(resolve('modules/tokenusage/ui/src/styles/layout.css'), 'utf8');
const rule = (selector) => {
  const start = css.indexOf(selector);
  assert.ok(start >= 0, `${selector} must exist`);
  return css.slice(start, css.indexOf('}', start) + 1);
};

// Closed dialogs must not be forced visible by the backdrop rule (the UA
// dialog:not([open]) display:none must survive).
assert.match(rule('.modal-backdrop {'), /display:\s*none/);
assert.match(rule('.modal-backdrop[open]'), /display:\s*flex/);

// Narrow windows keep a readable table and a reachable view switcher instead
// of squeezing headers into one character per line.
assert.match(css, /@media \(max-width:480px\)[\s\S]*?\.data-table \{min-width:560px;\}/);
assert.match(css, /\.period-tab \{[^}]*white-space:\s*nowrap/);
assert.match(rule('.view-btn.active'), /border-color:\s*var\(--natives-accent\)/);
assert.match(rule('.view-btn.active'), /box-shadow:\s*inset 0 -2px 0 var\(--natives-accent\)/);

// Both product themes must describe the selected state, not only a faint tint.
const theme = readFileSync(resolve('extension/theme.css'), 'utf8');
assert.match(theme, /--natives-selected:\s*rgba\(/);
console.log('tokenusage CSS: closed dialog, narrow table, active tab and theme roles passed');
