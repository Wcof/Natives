// Natives i18n — zh
// Thin composition entry: the exported `zh` object is built by spreading the
// per-domain dictionaries in ./zh/, keeping the exact same flat shape as before.

import { app } from './zh/app';
import { nav } from './zh/nav';
import { dashboard } from './zh/dashboard';
import { creative } from './zh/creative';
import { settings } from './zh/settings';
import { files } from './zh/files';
import { assistant } from './zh/assistant';
import { jobs } from './zh/jobs';

export const zh = {
  ...app,
  ...nav,
  ...dashboard,
  ...creative,
  ...settings,
  ...files,
  ...assistant,
  ...jobs,
};
