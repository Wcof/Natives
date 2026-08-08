// H0 fixture — root/js/module.js (ES module)
import './main.js';

fetch('../data/demo.json')
  .then((r) => r.json())
  .then((data) => console.log('[H0 module]', data))
  .catch((e) => console.error('[H0 module]', e));
