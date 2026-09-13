

import { WIDGET_KEYS, BACKGROUND_KEYS, POSITIONS, sanitizeHtml, escapeHtml } from './plugins/sanitizer.js';
import { backgroundPlugins } from './plugins/backgrounds/index.js';
import { widgetPlugins } from './plugins/widgets/index.js';

export { WIDGET_KEYS, BACKGROUND_KEYS, POSITIONS, sanitizeHtml, escapeHtml, backgroundPlugins, widgetPlugins };

const DEFAULT_ZH_NAMES = {
  'widget/aiPerformance': 'AI 效能（旧版）',
  // R1：七个独立 AI 效能组件的展示名。
  'widget/aiCost': 'AI 成本',
  'widget/aiTokens': 'Token 用量',
  'widget/aiSessions': '会话活跃',
  'widget/aiRequests': '调用统计',
  'widget/aiLimits': '额度与预算',
  'widget/aiAttention': '任务提醒',
  'widget/aiSavings': '降本建议',
  'widget/binaryTime': '二进制时钟',
  'widget/bookmarks': '书签',
  'widget/countdown': '倒计时',
  'widget/css': '自定义 CSS',
  'widget/currencyRates': '货币汇率',
  'widget/customText': '自定义文本',
  'widget/github': 'GitHub 日历',
  'widget/greeting': '问候语',
  'widget/html': '自定义 HTML',
  'widget/ipInfo': 'IP 信息',
  'widget/links': '快速链接',
  'widget/message': '消息',
  'widget/notes': '便签',
  'widget/palette': '随机调色板',
  'widget/quote': '名言',
  'widget/search': '搜索框',
  'widget/since': '时间跨度',
  'widget/tallyCounter': '计数器',
  'widget/time': '时间',
  'widget/todo': '待办事项',
  'widget/topSites': '常用站点',
  'widget/trello': 'Trello 看板',
  'widget/weather': '天气',
  'widget/workHours': '工作时间',
  'background/apod': '每日天文图片',
  'background/bing': 'Bing 每日壁纸',
  'background/colour': '纯色背景',
  'background/giphy': 'GIPHY',
  'background/gradient': '颜色渐变',
  'background/media': '上传图片和视频',
  'background/online': '在线图片',
  'background/unsplash': 'Unsplash',
  'background/wikimedia': 'Wikimedia 每日图片',
};

export function pluginName(key, language = 'en', fallback = key, t = null) {
  if (typeof t === 'function') {
    const localeKey = `plugin_${key.replace('/', '_')}`;
    const localized = t(localeKey, fallback);
    if (localized && localized !== localeKey) return localized;
  }
  if (language === 'zh_CN') {
    return DEFAULT_ZH_NAMES[key] || fallback;
  }
  return fallback;
}
