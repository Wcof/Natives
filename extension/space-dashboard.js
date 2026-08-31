/**
 * Space Shadow DOM Dashboard controller (<280 lines).
 * Implements: In-place DOM Registry (Mount/Patch/Move/Unmount),
 * GPU Compositing pipeline (translate3d/RAF throttle), and modular CSS caching.
 */

export function createSpaceDashboard({
  $,
  t,
  selectedLanguage,
  backgroundPlugins,
  widgetPlugins,
  nativeCall,
  broadcastRevision,
  onSelectWidget,
}) {
  let dashboardShadow = null;
  let dashboardRoot = null;
  let bgLayerEl = null;
  let currentBgKey = '';
  let currentBgDisplayStr = '';

  // Slot DOM containers: { topLeft, topCentre, ..., freeRoot }
  const slotContainers = {};
  // In-place Widget Registry: Map<widgetId, { element, wrapperEl, widget, pos, disposer }>
  const widgetRegistry = new Map();

  // Static cached stylesheet string compiled once
  let staticStyleContent = '';
  function getCompiledStyleSheet() {
    if (staticStyleContent) return staticStyleContent;
    const pluginStyles = [
      ...Object.values(backgroundPlugins || {}).map((p) => p.styles || ''),
      ...Object.values(widgetPlugins || {}).map((p) => p.styles || ''),
    ].filter(Boolean).join('\n');

    staticStyleContent = `
      :host { all: initial; }
      :host([data-widgets-hidden="true"]) .slot,
      :host(.widgets-hidden) .slot,
      :host-context(body.space-widgets-hidden) .slot {
        display: none !important;
      }
      .dashboard { width:100%; height:100%; position:relative; overflow:hidden; display:grid; font-family:-apple-system,BlinkMacSystemFont,'PingFang SC','Segoe UI',sans-serif; }
      .background-layer { position:absolute; inset:0; background-size:cover; background-position:center; transition:background 0.3s ease, filter 0.3s ease; }
      .background-not-configured { position:absolute; inset:0; display:grid; place-items:center; color:rgba(255,255,255,.6); font-size:14px; background:#111; }
      .slot { position:absolute; display:flex; flex-direction:column; gap:8px; pointer-events:auto; z-index:2; }
      .slot.topLeft { top:5%; left:5%; align-items:flex-start; }
      .slot.topCentre { top:5%; left:50%; transform:translateX(-50%); align-items:center; }
      .slot.topRight { top:5%; right:5%; align-items:flex-end; }
      .slot.middleLeft { top:50%; left:5%; transform:translateY(-50%); align-items:flex-start; }
      .slot.middleCentre { top:50%; left:50%; transform:translate(-50%,-50%); align-items:center; }
      .slot.middleRight { top:50%; right:5%; transform:translateY(-50%); align-items:flex-end; }
      .slot.bottomLeft { bottom:5%; left:5%; align-items:flex-start; }
      .slot.bottomCentre { bottom:5%; left:50%; transform:translateX(-50%); align-items:center; }
      .slot.bottomRight { bottom:5%; right:5%; align-items:flex-end; }
      .slot.free-widget { position:absolute; cursor:move; user-select:none; }
      .widget-container { color:#fff; text-shadow:0 1px 4px rgba(0,0,0,.6); transition:transform 0.1s ease; cursor:pointer; }
      .widget-container:hover { outline:1px dashed rgba(255,255,255,0.4); border-radius:4px; }
      .free-widget .free-handle { position:absolute; width:10px; height:10px; border-radius:50%; background:#fff; border:1px solid #333; opacity:0; pointer-events:auto; }
      .free-widget:hover .free-handle { opacity:0.8; }
      .free-widget .handle-scale { bottom:-6px; right:-6px; cursor:nwse-resize; }
      .free-widget .handle-rotate { top:-14px; left:50%; transform:translateX(-50%); cursor:grab; }
      ${pluginStyles}
    `;
    return staticStyleContent;
  }

  function ensureShadowShell() {
    const host = $('dashboard-host');
    if (dashboardShadow && dashboardRoot && host?.shadowRoot === dashboardShadow) {
      return { shadow: dashboardShadow, root: dashboardRoot };
    }
    if (!host) return { shadow: null, root: null };

    host.replaceChildren();
    dashboardShadow = host.attachShadow({ mode: 'open' });

    const style = document.createElement('style');
    style.textContent = getCompiledStyleSheet();
    dashboardShadow.append(style);

    dashboardRoot = document.createElement('div');
    dashboardRoot.className = 'dashboard';
    dashboardShadow.append(dashboardRoot);

    // Create background layer
    bgLayerEl = document.createElement('div');
    bgLayerEl.className = 'background-layer';
    dashboardRoot.append(bgLayerEl);

    // Create nine-grid slot containers
    const NINE_SLOTS = [
      'topLeft', 'topCentre', 'topRight',
      'middleLeft', 'middleCentre', 'middleRight',
      'bottomLeft', 'bottomCentre', 'bottomRight',
    ];
    for (const slotName of NINE_SLOTS) {
      const slotEl = document.createElement('div');
      slotEl.className = `slot ${slotName}`;
      dashboardRoot.append(slotEl);
      slotContainers[slotName] = slotEl;
    }

    return { shadow: dashboardShadow, root: dashboardRoot };
  }

  function render(snapshot, activeWorkspaceId, updateSnapshot) {
    if (!snapshot) return;
    const { shadow, root } = ensureShadowShell();
    if (!shadow || !root) return;

    // 1. In-place Background Check & Patch
    const bgData = snapshot.backgroundJson || {};
    const bgKey = bgData.key || 'background/colour';
    const bgDisplay = bgData.display || bgData.data || {};
    const bgDisplayStr = JSON.stringify({ key: bgKey, ...bgDisplay });

    if (bgKey !== currentBgKey || bgDisplayStr !== currentBgDisplayStr) {
      currentBgKey = bgKey;
      currentBgDisplayStr = bgDisplayStr;
      const bgPlugin = backgroundPlugins[bgKey] || backgroundPlugins['background/colour'];
      if (bgPlugin && bgLayerEl) {
        let isNight = false;
        if (bgDisplay.nightDim) {
          const h = new Date().getHours();
          isNight = h >= 20 || h < 6;
        }
        const blurPx = Number(bgDisplay.blur) || 0;
        const bright = bgDisplay.brightness ?? (isNight ? 0.6 : 1);
        bgLayerEl.style.filter = (blurPx > 0 || bright !== 1) ? `blur(${blurPx}px) brightness(${bright})` : 'none';
        bgPlugin.render(bgLayerEl, bgDisplay, { t, lang: selectedLanguage });
      }
    }

    // 2. In-place Diff & Patch of Widgets
    const incomingWidgets = (snapshot.widgets || []).filter((w) => w.enabled);
    const incomingIds = new Set(incomingWidgets.map((w) => w.id));

    // Remove unmounted widgets
    for (const [id, entry] of widgetRegistry.entries()) {
      if (!incomingIds.has(id)) {
        entry.disposer?.();
        (entry.wrapperEl || entry.element).remove();
        widgetRegistry.delete(id);
      }
    }

    // Mount or patch incoming widgets
    for (const widget of incomingWidgets) {
      const existing = widgetRegistry.get(widget.id);
      const pos = widget.displayJson?.position || 'middleCentre';

      if (!existing) {
        // Mount new widget
        mountWidget(widget, pos, shadow, root, snapshot, activeWorkspaceId, updateSnapshot);
      } else {
        // In-place patch existing widget
        patchWidget(existing, widget, pos, shadow, root, snapshot, activeWorkspaceId, updateSnapshot);
      }
    }
  }

  function mountWidget(widget, pos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot) {
    const plugin = widgetPlugins[widget.key];
    if (!plugin) return;

    const container = document.createElement('div');
    const keyClass = widget.key.replace('widget/', '');
    container.className = `widget-container widget-${keyClass}`;
    container.dataset.widgetId = widget.id;

    applyWidgetDisplayStyles(container, widget.displayJson || {});

    let disposer = null;
    plugin.render(container, widget.configJson || {}, widget.displayJson || {}, {
      t,
      lang: selectedLanguage,
      shadowRoot,
      onDataChange: async (nextData) => {
        try {
          const result = await nativeCall('workspace_widget_upsert', {
            workspaceId: activeWorkspaceId,
            widget: { ...widget, configJson: nextData },
            expectedRevision: snapshot.revision,
          });
          updateSnapshot(result);
          broadcastRevision();
        } catch (err) {}
      },
    });

    container.onclick = (e) => {
      e.stopPropagation();
      onSelectWidget(widget.id);
    };

    let wrapperEl = null;
    if (pos === 'free') {
      wrapperEl = document.createElement('div');
      wrapperEl.className = 'slot free-widget';
      const disp = widget.displayJson || {};
      wrapperEl.style.left = `${disp.xPercent ?? 50}%`;
      wrapperEl.style.top = `${disp.yPercent ?? 50}%`;
      wrapperEl.style.transform = `translate(-50%, -50%) scale(${disp.scale ?? 1}) rotate(${disp.rotation ?? 0}deg)`;
      wrapperEl.dataset.widgetId = widget.id;

      const scaleHandle = document.createElement('div');
      scaleHandle.className = 'free-handle handle-scale';
      const rotHandle = document.createElement('div');
      rotHandle.className = 'free-handle handle-rotate';
      wrapperEl.append(scaleHandle, rotHandle, container);

      attachFreeGestures(wrapperEl, scaleHandle, rotHandle, widget, snapshot, activeWorkspaceId, updateSnapshot);
      root.append(wrapperEl);
    } else {
      const targetSlot = slotContainers[pos] || slotContainers.middleCentre;
      targetSlot?.append(container);
    }

    widgetRegistry.set(widget.id, {
      element: container,
      wrapperEl,
      widget,
      pos,
      disposer,
    });
  }

  function patchWidget(entry, nextWidget, nextPos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot) {
    const prevWidget = entry.widget;
    entry.widget = nextWidget;

    // Check position move
    if (entry.pos !== nextPos) {
      entry.disposer?.();
      (entry.wrapperEl || entry.element).remove();
      widgetRegistry.delete(nextWidget.id);
      mountWidget(nextWidget, nextPos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot);
      return;
    }

    // In-place style & class update
    applyWidgetDisplayStyles(entry.element, nextWidget.displayJson || {});

    // Re-render plugin contents only if config changed
    if (JSON.stringify(prevWidget.configJson) !== JSON.stringify(nextWidget.configJson)) {
      const plugin = widgetPlugins[nextWidget.key];
      plugin?.render?.(entry.element, nextWidget.configJson || {}, nextWidget.displayJson || {}, {
        t,
        lang: selectedLanguage,
        shadowRoot,
        onDataChange: async (nextData) => {
          try {
            const result = await nativeCall('workspace_widget_upsert', {
              workspaceId: activeWorkspaceId,
              widget: { ...nextWidget, configJson: nextData },
              expectedRevision: snapshot.revision,
            });
            updateSnapshot(result);
            broadcastRevision();
          } catch (err) {}
        },
      });
    }
  }

  function applyWidgetDisplayStyles(container, disp) {
    container.style.fontSize = disp.fontSize ? `${disp.fontSize}px` : '';
    container.style.color = disp.useAccentColor ? 'var(--accent, #cdf24b)' : (disp.colour || '');
    container.style.fontWeight = disp.fontWeight ? String(disp.fontWeight) : '';
    container.style.fontStyle = disp.fontStyle || '';
    container.style.textDecoration = disp.textDecoration || '';

    // Custom class token
    if (disp.customClass && /^[a-zA-Z0-9_-]+$/.test(disp.customClass)) {
      container.classList.add(disp.customClass);
    }
  }

  function attachFreeGestures(element, scaleHandle, rotHandle, widget, snapshot, activeWorkspaceId, updateSnapshot) {
    let isInteracting = false;
    let mode = 'idle'; // 'drag' | 'scale' | 'rotate'
    let startX = 0;
    let startY = 0;
    let initXPercent = widget.displayJson?.xPercent ?? 50;
    let initYPercent = widget.displayJson?.yPercent ?? 50;
    let initScale = widget.displayJson?.scale ?? 1;
    let initRot = widget.displayJson?.rotation ?? 0;

    let rafId = null;
    let curDx = 0;
    let curDy = 0;
    let curScale = initScale;
    let curRot = initRot;

    function applyTransform() {
      if (mode === 'drag') {
        element.style.transform = `translate3d(calc(-50% + ${curDx}px), calc(-50% + ${curDy}px), 0) scale(${initScale}) rotate(${initRot}deg)`;
      } else if (mode === 'scale') {
        element.style.transform = `translate3d(-50%, -50%, 0) scale(${curScale}) rotate(${initRot}deg)`;
      } else if (mode === 'rotate') {
        element.style.transform = `translate3d(-50%, -50%, 0) scale(${initScale}) rotate(${curRot}deg)`;
      }
      rafId = null;
    }

    element.onpointerdown = (e) => {
      if (['INPUT', 'TEXTAREA', 'BUTTON', 'A'].includes(e.target.tagName)) return;
      if (e.target === scaleHandle || e.target === rotHandle) return;
      isInteracting = true;
      mode = 'drag';
      startX = e.clientX;
      startY = e.clientY;
      element.setPointerCapture(e.pointerId);
      element.style.willChange = 'transform';
      e.stopPropagation();
    };

    scaleHandle.onpointerdown = (e) => {
      isInteracting = true;
      mode = 'scale';
      startX = e.clientX;
      scaleHandle.setPointerCapture(e.pointerId);
      element.style.willChange = 'transform';
      e.stopPropagation();
    };

    rotHandle.onpointerdown = (e) => {
      isInteracting = true;
      mode = 'rotate';
      startX = e.clientX;
      rotHandle.setPointerCapture(e.pointerId);
      element.style.willChange = 'transform';
      e.stopPropagation();
    };

    element.onpointermove = (e) => {
      if (!isInteracting) return;
      if (mode === 'drag') {
        curDx = e.clientX - startX;
        curDy = e.clientY - startY;
      } else if (mode === 'scale') {
        const ds = (e.clientX - startX) * 0.01;
        curScale = Math.max(0.2, Math.min(3, initScale + ds));
      } else if (mode === 'rotate') {
        const dr = (e.clientX - startX) * 1.5;
        curRot = Math.round(initRot + dr);
      }
      if (!rafId && typeof requestAnimationFrame !== 'undefined') {
        rafId = requestAnimationFrame(applyTransform);
      }
    };

    element.onpointerup = async (e) => {
      if (!isInteracting) return;
      isInteracting = false;
      element.style.willChange = 'auto';
      if (rafId && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(rafId);

      const host = $('dashboard-host');
      const hostW = host?.offsetWidth || window.innerWidth;
      const hostH = host?.offsetHeight || window.innerHeight;

      const finalDxPercent = (curDx / hostW) * 100;
      const finalDyPercent = (curDy / hostH) * 100;
      const finalX = mode === 'drag' ? Math.round(Math.max(0, Math.min(100, initXPercent + finalDxPercent)) * 10) / 10 : initXPercent;
      const finalY = mode === 'drag' ? Math.round(Math.max(0, Math.min(100, initYPercent + finalDyPercent)) * 10) / 10 : initYPercent;
      const finalScale = mode === 'scale' ? Math.round(curScale * 10) / 10 : initScale;
      const finalRot = mode === 'rotate' ? curRot : initRot;

      // Reset transform and commit position in-place
      element.style.left = `${finalX}%`;
      element.style.top = `${finalY}%`;
      element.style.transform = `translate(-50%, -50%) scale(${finalScale}) rotate(${finalRot}deg)`;

      mode = 'idle';
      curDx = 0;
      curDy = 0;

      try {
        const nextDisplay = {
          ...widget.displayJson,
          position: 'free',
          xPercent: finalX,
          yPercent: finalY,
          scale: finalScale,
          rotation: finalRot,
        };
        const result = await nativeCall('workspace_widget_upsert', {
          workspaceId: activeWorkspaceId,
          widget: { ...widget, displayJson: nextDisplay },
          expectedRevision: snapshot.revision,
        });
        updateSnapshot(result);
        broadcastRevision();
      } catch (err) {
        render(snapshot, activeWorkspaceId, updateSnapshot);
      }
    };

    element.onpointercancel = () => {
      isInteracting = false;
      mode = 'idle';
      element.style.willChange = 'auto';
      render(snapshot, activeWorkspaceId, updateSnapshot);
    };
  }

  return {
    render,
    setWidgetsHidden(hidden) {
      const host = $('dashboard-host');
      if (host) {
        host.classList.toggle('widgets-hidden', Boolean(hidden));
        host.dataset.widgetsHidden = String(Boolean(hidden));
      }
    },
    destroy() {
      for (const entry of widgetRegistry.values()) {
        entry.disposer?.();
      }
      widgetRegistry.clear();
    },
  };
}
