// apps.html 页面装配入口（mode/T06，R-E3/R-M6）：页面级装配负责注入
// openSettings；App Center 嵌入 Shell 与 Broker 不回导上层设置页。本模块
// 是装配汇点（无任何 Shell 链反向导入），导入图保持无环。
import { ensureAuthenticated } from './platform/web-session.js';
import { createAppCenter } from './apps.js';
import { openModelSettings } from './model-settings.js';
import { initI18n } from './platform/i18n.js';
import { storageGet } from './files-preferences.js';

if (typeof document !== 'undefined' && document.getElementById('apps-list')) {
  ensureAuthenticated().then(async (ok) => {
    if (!ok) return;
    const language=await storageGet('natives-language','zh_CN');
    await initI18n(language);
    createAppCenter({
      language,
      openSettings: (context) => openModelSettings(context),
    });
  });
}
