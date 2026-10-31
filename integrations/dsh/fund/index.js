import { registerUrlRoute } from './runtime.js';

/** Host half: one read-only same-origin route returning the space page URL. */
export function apply(ctx) {
  ctx.inject(['webServer'], (scoped) => {
    registerUrlRoute(scoped, { route: '/natives/fund/url', target: 'fund' });
  });
}
