// app.html 页面装配入口（mode/T06，R-E3/R-M6）：页面级装配负责注入
// openSettings；Shell／Broker／client 不回导上层设置页。本模块是装配
// 汇点（无任何 Shell 链反向导入），导入图保持无环。
import { ensureAuthenticated } from './platform/web-session.js';
import { bootAppPage } from './app.js';
import { openModelSettings } from './model-settings.js';
import { getMessage } from './platform/i18n.js';

if (typeof document !== 'undefined' && globalThis.location?.pathname?.endsWith('app.html') && document.getElementById('app-stage')) {
  ensureAuthenticated().then((ok) => {
    if (!ok) return;
    return bootAppPage({
      openSettings: (context) => openModelSettings(context),
    });
  }).catch(() => {
    const toast = document.getElementById('app-toast');
    if (toast) {
      toast.hidden = false;
      toast.textContent = getMessage('appOpenFailed') || 'appOpenFailed';
    }
  });
}
