(() => {
  if (window.__nativesScreenshot?.active) {
    return;
  }

  let originalScrollX = 0;
  let originalScrollY = 0;
  let scrollbarStyleEl = null;
  let fixedElements = [];
  let isRestored = false;

  function measure() {
    originalScrollX = window.scrollX || window.pageXOffset || document.documentElement.scrollLeft || 0;
    originalScrollY = window.scrollY || window.pageYOffset || document.documentElement.scrollTop || 0;

    // Inject scrollbar-hiding style to avoid capturing scrollbars in screenshots
    if (!scrollbarStyleEl) {
      scrollbarStyleEl = document.createElement('style');
      scrollbarStyleEl.id = '__natives_screenshot_scrollbar__';
      scrollbarStyleEl.textContent = `
        ::-webkit-scrollbar { display: none !important; width: 0 !important; height: 0 !important; }
        html, body { scrollbar-width: none !important; }
      `;
      (document.head || document.documentElement).appendChild(scrollbarStyleEl);
    }

    // Discover fixed and sticky elements
    fixedElements = [];
    const all = document.querySelectorAll('*');
    for (const el of all) {
      if (el === scrollbarStyleEl || el.id === '__natives_screenshot_scrollbar__') continue;
      try {
        const pos = window.getComputedStyle(el).position;
        if (pos === 'fixed' || pos === 'sticky') {
          fixedElements.push({
            element: el,
            originalVisibility: el.style.visibility || '',
          });
        }
      } catch {
        // ignore detached or inaccessible elements
      }
    }

    const docEl = document.documentElement || {};
    const body = document.body || {};

    const viewportWidth = window.innerWidth || docEl.clientWidth || 1;
    const viewportHeight = window.innerHeight || docEl.clientHeight || 1;

    const totalWidth = Math.max(
      docEl.scrollWidth || 0,
      body.scrollWidth || 0,
      docEl.clientWidth || 0,
      docEl.offsetWidth || 0,
      body.offsetWidth || 0,
      viewportWidth
    );

    const totalHeight = Math.max(
      docEl.scrollHeight || 0,
      body.scrollHeight || 0,
      docEl.clientHeight || 0,
      docEl.offsetHeight || 0,
      body.offsetHeight || 0,
      viewportHeight
    );

    const dpr = window.devicePixelRatio || 1;
    const title = document.title || 'screenshot';

    window.addEventListener('beforeunload', restore, { once: true });
    window.addEventListener('pagehide', restore, { once: true });

    return {
      viewportWidth,
      viewportHeight,
      totalWidth,
      totalHeight,
      dpr,
      title,
      url: location.href,
    };
  }

  function scrollTo(x, y, hideFixed) {
    if (hideFixed) {
      for (const item of fixedElements) {
        try {
          item.element.style.visibility = 'hidden';
        } catch {
          // ignore
        }
      }
    }

    window.scrollTo(x, y);

    // Trigger scroll event so lazy-loaded images or observers fire
    try {
      window.dispatchEvent(new Event('scroll'));
    } catch {
      // ignore
    }
    return true;
  }

  function restore() {
    if (isRestored) return;
    isRestored = true;

    if (scrollbarStyleEl && scrollbarStyleEl.parentNode) {
      scrollbarStyleEl.parentNode.removeChild(scrollbarStyleEl);
      scrollbarStyleEl = null;
    }

    for (const item of fixedElements) {
      try {
        item.element.style.visibility = item.originalVisibility;
      } catch {
        // ignore
      }
    }
    fixedElements = [];

    window.scrollTo(originalScrollX, originalScrollY);

    window.removeEventListener('beforeunload', restore);
    window.removeEventListener('pagehide', restore);

    delete window.__nativesScreenshot;
  }

  window.__nativesScreenshot = {
    active: true,
    measure,
    scrollTo,
    restore,
  };
})();
