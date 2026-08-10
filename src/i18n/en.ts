// Natives i18n — en
// Thin composition entry: the exported `en` object is built by spreading the
// per-domain dictionaries in ./en/, keeping the exact same flat shape as before.

import { app } from './en/app';
import { nav } from './en/nav';
import { dashboard } from './en/dashboard';
import { creative } from './en/creative';
import { settings } from './en/settings';
import { files } from './en/files';
import { assistant } from './en/assistant';
import { jobs } from './en/jobs';

export const en = {
  ...app,
  ...nav,
  ...dashboard,
  ...creative,
  ...settings,
  ...files,
  ...assistant,
  ...jobs,
};
