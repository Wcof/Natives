

import { apodBackground } from './apod.js';
import { bingBackground } from './bing.js';
import { colourBackground } from './colour.js';
import { giphyBackground } from './giphy.js';
import { gradientBackground } from './gradient.js';
import { mediaBackground } from './media.js';
import { onlineBackground } from './online.js';
import { unsplashBackground } from './unsplash.js';
import { wikimediaBackground } from './wikimedia.js';

export const backgroundPlugins = {
  [apodBackground.key]: apodBackground,
  [bingBackground.key]: bingBackground,
  [colourBackground.key]: colourBackground,
  [giphyBackground.key]: giphyBackground,
  [gradientBackground.key]: gradientBackground,
  [mediaBackground.key]: mediaBackground,
  [onlineBackground.key]: onlineBackground,
  [unsplashBackground.key]: unsplashBackground,
  [wikimediaBackground.key]: wikimediaBackground,
};
