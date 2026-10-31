/**
 * 空间卡片元数据：目录分类、可配置项与新卡片的展示名。
 *
 * 这里是「卡片有哪些 key」的唯一前端来源：
 *   - plugins/sanitizer.js 直接 re-export WIDGET_KEYS；
 *   - space-catalog.js 按 CARD_CATEGORIES 生成目录；
 *   - space-widget-settings.js 按 limit 生成通用「显示数量」控件。
 *
 * Rust Host 仍持有一份白名单（crates/native-file-host/src/workspace_store/schema.rs），
 * 两边必须同步；space-kit.test.mjs 会读取 Rust 源码做一致性校验。
 */

/** 分类顺序即目录分组顺序。 */
export const CARD_CATEGORIES = [
  { id: 'investment', nameKey: 'catInvestment', fallback: '投资' },
  { id: 'ai', nameKey: 'catAiPerformance', fallback: 'AI 效能' },
  { id: 'time', nameKey: 'catTimeAndClocks', fallback: '时间与时钟' },
  { id: 'productivity', nameKey: 'catProductivity', fallback: '生产力与工具' },
  { id: 'information', nameKey: 'catInformationFeeds', fallback: '资讯与数据' },
  { id: 'inspiration', nameKey: 'catInspirationText', fallback: '灵感与问候' },
  { id: 'advanced', nameKey: 'catCustomAdvanced', fallback: '自定义与扩展' },
];

/**
 * 卡片定义。
 * category         目录分组
 * hidden           不在目录里展示（兼容旧 key 使用）
 * name/nameZh      新卡片展示名；老卡片继续走 locale / DEFAULT_ZH_NAMES
 * description      目录说明（英文），descriptionZh 为中文
 * icon             card-shell 的图标名
 * defaultPosition  从目录添加时的初始九宫格位置
 * limit            { def, max } 通用「显示数量」配置
 */
export const CARD_META = {
  // 投资（ADR-0036）
  'widget/investmentHoldings': { category: 'investment' },
  'widget/investmentAssets': { category: 'investment' },
  'widget/investmentGoal': { category: 'investment' },
  'widget/investmentAllocation': { category: 'investment' },
  'widget/investmentAllocationCheck': { category: 'investment' },

  // AI 效能（R1）
  'widget/aiPerformance': { category: 'ai', hidden: true },
  'widget/aiCost': { category: 'ai' },
  'widget/aiTokens': { category: 'ai' },
  'widget/aiSessions': { category: 'ai' },
  'widget/aiRequests': { category: 'ai' },
  'widget/aiLimits': { category: 'ai' },
  'widget/aiAttention': { category: 'ai' },
  'widget/aiSavings': { category: 'ai' },

  // 时间
  'widget/time': { category: 'time' },
  'widget/binaryTime': { category: 'time' },
  'widget/since': { category: 'time' },
  'widget/countdown': { category: 'time' },
  'widget/workHours': { category: 'time' },
  'widget/timeProgress': {
    category: 'time',
    icon: 'gauge',
    name: 'Time Progress',
    nameZh: '时间进度',
    description: 'Elapsed progress for today, this week, this month and this year',
    descriptionZh: '今天、本周、本月、今年的流逝进度',
    defaultPosition: 'bottomRight',
  },
  'widget/worldClock': {
    category: 'time',
    icon: 'globe',
    name: 'World Clock',
    nameZh: '世界时钟',
    description: 'Compare several time zones at a glance',
    descriptionZh: '并排查看多个时区',
    defaultPosition: 'middleRight',
  },

  // 生产力与工具
  'widget/todo': { category: 'productivity' },
  'widget/notes': { category: 'productivity' },
  'widget/trello': { category: 'productivity' },
  'widget/tallyCounter': { category: 'productivity' },
  'widget/search': { category: 'productivity' },
  'widget/links': { category: 'productivity' },
  'widget/bookmarks': { category: 'productivity' },
  'widget/topSites': { category: 'productivity' },
  'widget/dailyFocus': {
    category: 'productivity',
    icon: 'target',
    name: 'Daily Focus',
    nameZh: '今日重点',
    description: "Keep today's three most important tasks in view",
    descriptionZh: '每天只保留最重要的三件事',
    defaultPosition: 'topLeft',
    limit: { def: 3, max: 6 },
  },
  'widget/quickNote': {
    category: 'productivity',
    icon: 'pen',
    name: 'Quick Notes',
    nameZh: '快速记录',
    description: 'Capture fleeting thoughts with timestamps',
    descriptionZh: '用时间戳记下临时想法',
    defaultPosition: 'topRight',
    limit: { def: 5, max: 10 },
  },
  'widget/scratchpad': {
    category: 'productivity',
    icon: 'notebook',
    name: 'Scratchpad',
    nameZh: '便签',
    description: 'A free-form note that saves itself',
    descriptionZh: '一块自动保存的自由便签',
    defaultPosition: 'middleLeft',
  },
  'widget/habit': {
    category: 'productivity',
    icon: 'flame',
    name: 'Habits',
    nameZh: '习惯打卡',
    description: "A seven-day check-in grid and today's progress",
    descriptionZh: '七天打卡网格与今日进度',
    defaultPosition: 'bottomLeft',
  },
  'widget/dailyQuestion': {
    category: 'productivity',
    icon: 'help',
    name: 'Daily Question',
    nameZh: '每日一问',
    description: 'One question a day, answers saved by date',
    descriptionZh: '每天一个问题，答案按日保存',
    defaultPosition: 'bottomCentre',
  },

  // 资讯与数据
  'widget/weather': { category: 'information' },
  'widget/currencyRates': { category: 'information' },
  'widget/ipInfo': { category: 'information' },
  'widget/github': { category: 'information' },

  // 灵感与问候
  'widget/greeting': { category: 'inspiration' },
  'widget/quote': { category: 'inspiration' },
  'widget/palette': { category: 'inspiration' },
  'widget/message': { category: 'inspiration' },
  'widget/customText': { category: 'inspiration' },

  // 自定义与扩展
  'widget/html': { category: 'advanced' },
  'widget/css': { category: 'advanced' },
};

/** 所有合法 key，顺序与目录定义一致。 */
export const WIDGET_KEYS = Object.freeze(Object.keys(CARD_META));

const META_BY_KEY = new Map(Object.entries(CARD_META));

/** 查元数据；未知 key 返回 undefined。 */
export function cardMeta(key) {
  return META_BY_KEY.get(key);
}

/** 目录里可见的 key。 */
export function catalogWidgetKeys() {
  return WIDGET_KEYS.filter((key) => !CARD_META[key]?.hidden);
}

/** 某个分类下的 key（保持 CARD_META 声明顺序）。 */
export function cardsInCategory(categoryId) {
  return catalogWidgetKeys().filter((key) => CARD_META[key].category === categoryId);
}

/** 把任意输入夹到卡片声明的 limit 范围内；未声明 limit 时返回 undefined。 */
export function normalizeCardLimit(key, value) {
  const limit = CARD_META[key]?.limit;
  if (!limit) return undefined;
  const number = Math.floor(Number(value));
  if (!Number.isFinite(number)) return limit.def;
  return Math.max(1, Math.min(limit.max, number));
}
