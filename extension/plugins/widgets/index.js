
import { aiComponentDefinitions, aiPerformanceWidget } from '../../ai-performance/widget.js';
import { binaryTimeWidget } from './binary-time.js';
import { bookmarksWidget } from './bookmarks.js';
import { countdownWidget } from './countdown.js';
import { cssWidget } from './css.js';
import { currencyRatesWidget } from './currency-rates.js';
import { customTextWidget } from './custom-text.js';
import { dailyFocusWidget } from './daily-focus.js';
import { dailyQuestionWidget } from './daily-question.js';
import { githubWidget } from './github.js';
import { greetingWidget } from './greeting.js';
import { habitWidget } from './habit.js';
import { htmlWidget } from './html.js';
import { ipInfoWidget } from './ip-info.js';
import { linksWidget } from './links.js';
import { investmentAllocationCheckWidget, investmentAllocationWidget } from './investment-allocation.js';
import { investmentAssetsWidget } from './investment-assets.js';
import { investmentGoalWidget } from './investment-goal.js';
import { investmentHoldingsWidget } from './investment-holdings.js';
import { messageWidget } from './message.js';
import { notesWidget } from './notes.js';
import { paletteWidget } from './palette.js';
import { quickNoteWidget } from './quick-note.js';
import { quoteWidget } from './quote.js';
import { scratchpadWidget } from './scratchpad.js';
import { searchWidget } from './search.js';
import { sinceWidget } from './since.js';
import { tallyCounterWidget } from './tally-counter.js';
import { timeProgressWidget } from './time-progress.js';
import { timeWidget } from './time.js';
import { todoWidget } from './todo.js';
import { topSitesWidget } from './top-sites.js';
import { trelloWidget } from './trello.js';
import { weatherWidget } from './weather.js';
import { workHoursWidget } from './work-hours.js';
import { worldClockWidget } from './world-clock.js';

export const widgetPlugins = {
  [aiPerformanceWidget.key]: aiPerformanceWidget,
  // 投资卡（ADR-0036）：全部消费页面级一次只读投影，无独立查询。
  [investmentHoldingsWidget.key]: investmentHoldingsWidget,
  [investmentAssetsWidget.key]: investmentAssetsWidget,
  [investmentGoalWidget.key]: investmentGoalWidget,
  [investmentAllocationWidget.key]: investmentAllocationWidget,
  [investmentAllocationCheckWidget.key]: investmentAllocationCheckWidget,
  // R1：七个独立 AI 效能组件（与旧 aiPerformance 共享 renderer/查询/样式）。
  [aiComponentDefinitions.aiCost.key]: aiComponentDefinitions.aiCost,
  [aiComponentDefinitions.aiTokens.key]: aiComponentDefinitions.aiTokens,
  [aiComponentDefinitions.aiSessions.key]: aiComponentDefinitions.aiSessions,
  [aiComponentDefinitions.aiRequests.key]: aiComponentDefinitions.aiRequests,
  [aiComponentDefinitions.aiLimits.key]: aiComponentDefinitions.aiLimits,
  [aiComponentDefinitions.aiAttention.key]: aiComponentDefinitions.aiAttention,
  [aiComponentDefinitions.aiSavings.key]: aiComponentDefinitions.aiSavings,
  [binaryTimeWidget.key]: binaryTimeWidget,
  [bookmarksWidget.key]: bookmarksWidget,
  [countdownWidget.key]: countdownWidget,
  [cssWidget.key]: cssWidget,
  [currencyRatesWidget.key]: currencyRatesWidget,
  [customTextWidget.key]: customTextWidget,
  [dailyFocusWidget.key]: dailyFocusWidget,
  [dailyQuestionWidget.key]: dailyQuestionWidget,
  [githubWidget.key]: githubWidget,
  [greetingWidget.key]: greetingWidget,
  [habitWidget.key]: habitWidget,
  [htmlWidget.key]: htmlWidget,
  [ipInfoWidget.key]: ipInfoWidget,
  [linksWidget.key]: linksWidget,
  [messageWidget.key]: messageWidget,
  [notesWidget.key]: notesWidget,
  [paletteWidget.key]: paletteWidget,
  [quickNoteWidget.key]: quickNoteWidget,
  [quoteWidget.key]: quoteWidget,
  [scratchpadWidget.key]: scratchpadWidget,
  [searchWidget.key]: searchWidget,
  [sinceWidget.key]: sinceWidget,
  [tallyCounterWidget.key]: tallyCounterWidget,
  [timeProgressWidget.key]: timeProgressWidget,
  [timeWidget.key]: timeWidget,
  [todoWidget.key]: todoWidget,
  [topSitesWidget.key]: topSitesWidget,
  [trelloWidget.key]: trelloWidget,
  [weatherWidget.key]: weatherWidget,
  [workHoursWidget.key]: workHoursWidget,
  [worldClockWidget.key]: worldClockWidget,
};
