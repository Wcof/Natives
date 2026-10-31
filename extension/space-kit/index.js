/**
 * space-kit 公共入口。
 *
 * 约束：
 *   - 只允许 cards 依赖 space-kit，不允许 space-kit 反向依赖 dashboard / inspector / space.js；
 *   - 不读取 workspace 快照，不直接调用 Native Host。
 */

export {
  CARD_CATEGORIES,
  CARD_META,
  WIDGET_KEYS,
  cardMeta,
  cardsInCategory,
  catalogWidgetKeys,
  normalizeCardLimit,
} from './card-meta.js';

export {
  clampInt,
  dayKey,
  parseDayKey,
  daysBetween,
  lastNDays,
  timeProgress,
  dailyIndex,
  zonedClock,
  formatStamp,
  pruneDayMap,
  capText,
  createId,
} from './card-data.js';

export { createCardShell, createCardAction } from './card-shell.js';
export { createIcon, hasIcon } from './card-icons.js';
export { SPACE_CARD_STYLES } from './card-styles.js';

export {
  el,
  createEmptyState,
  createInlineForm,
  createListRow,
  createProgressRow,
  createTextarea,
  createStatus,
  createChip,
  createDebouncedCommit,
} from './card-ui.js';
