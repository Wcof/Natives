/**
 * Registry aggregator for 24 TablissNG Chromium widgets.
 */

import { binaryTimeWidget } from './binary-time.js';
import { bookmarksWidget } from './bookmarks.js';
import { countdownWidget } from './countdown.js';
import { cssWidget } from './css.js';
import { currencyRatesWidget } from './currency-rates.js';
import { customTextWidget } from './custom-text.js';
import { githubWidget } from './github.js';
import { greetingWidget } from './greeting.js';
import { htmlWidget } from './html.js';
import { ipInfoWidget } from './ip-info.js';
import { linksWidget } from './links.js';
import { messageWidget } from './message.js';
import { notesWidget } from './notes.js';
import { paletteWidget } from './palette.js';
import { quoteWidget } from './quote.js';
import { searchWidget } from './search.js';
import { sinceWidget } from './since.js';
import { tallyCounterWidget } from './tally-counter.js';
import { timeWidget } from './time.js';
import { todoWidget } from './todo.js';
import { topSitesWidget } from './top-sites.js';
import { trelloWidget } from './trello.js';
import { weatherWidget } from './weather.js';
import { workHoursWidget } from './work-hours.js';

export const widgetPlugins = {
  [binaryTimeWidget.key]: binaryTimeWidget,
  [bookmarksWidget.key]: bookmarksWidget,
  [countdownWidget.key]: countdownWidget,
  [cssWidget.key]: cssWidget,
  [currencyRatesWidget.key]: currencyRatesWidget,
  [customTextWidget.key]: customTextWidget,
  [githubWidget.key]: githubWidget,
  [greetingWidget.key]: greetingWidget,
  [htmlWidget.key]: htmlWidget,
  [ipInfoWidget.key]: ipInfoWidget,
  [linksWidget.key]: linksWidget,
  [messageWidget.key]: messageWidget,
  [notesWidget.key]: notesWidget,
  [paletteWidget.key]: paletteWidget,
  [quoteWidget.key]: quoteWidget,
  [searchWidget.key]: searchWidget,
  [sinceWidget.key]: sinceWidget,
  [tallyCounterWidget.key]: tallyCounterWidget,
  [timeWidget.key]: timeWidget,
  [todoWidget.key]: todoWidget,
  [topSitesWidget.key]: topSitesWidget,
  [trelloWidget.key]: trelloWidget,
  [weatherWidget.key]: weatherWidget,
  [workHoursWidget.key]: workHoursWidget,
};
