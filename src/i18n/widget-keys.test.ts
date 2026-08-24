// WS-01: every registered widget's titleKey (and descriptionKey when present)
// must resolve to a localized string in BOTH zh and en, and the resolved value
// must never equal the key itself (the "bare key" regression this test guards).

import assert from 'node:assert/strict';
import test from 'node:test';
import { t } from './index';
// Importing the components barrel registers all built-in widgets as a side
// effect; the registry is read from the lib module that owns it.
import '../components/workspace/widgets';
import { getAllWidgets } from '@/lib/workspace/widgets';

test('registered widget title/description keys resolve in zh and en without falling back to the key', () => {
  // Importing the barrel triggers widget registration (side effect).
  const defs = getAllWidgets();
  assert.ok(defs.length >= 16, `expected ≥16 registered widgets, got ${defs.length}`);

  for (const def of defs) {
    const titleKeys = [def.titleKey];
    if (def.descriptionKey && def.descriptionKey !== def.titleKey) {
      titleKeys.push(def.descriptionKey);
    }
    for (const key of titleKeys) {
      for (const locale of ['zh', 'en'] as const) {
        const resolved = t(locale, key);
        assert.notEqual(
          resolved,
          key,
          `widget "${def.type}" key "${key}" is missing in ${locale} (resolved to the bare key)`,
        );
        assert.ok(
          typeof resolved === 'string' && resolved.trim().length > 0,
          `widget "${def.type}" key "${key}" resolved to an empty value in ${locale}`,
        );
      }
    }
  }
});
