/**
 * Space Shadow DOM Dashboard controller (<290 lines).
 * Full implementation of Tabliss Widgets.sass and Slot.sass.
 * Separates direct widget interactive usage from explicit position editing.
 */

const transformOriginMap = {
  topLeft: 'top left',
  topCentre: 'top center',
  topRight: 'top right',
  middleLeft: 'center left',
  middleCentre: 'center center',
  middleRight: 'center right',
  bottomLeft: 'bottom left',
  bottomCentre: 'bottom center',
  bottomRight: 'bottom right',
  free: 'center center',
};

export function createSpaceDashboard({
  $,
  t,
  selectedLanguage,
  backgroundPlugins,
  widgetPlugins,
  nativeCall,
  broadcastRevision,
}) {
  let dashboardShadow = null;
  let dashboardRoot = null;
  let bgLayerEl = null;
  let currentBgKey = '';
  let currentBgDisplayStr = '';
  let currentEditingWidgetId = null;

  const slotContainers = {};
  const widgetRegistry = new Map();

  let staticStyleContent = '';
  function getCompiledStyleSheet() {
    if (staticStyleContent) return staticStyleContent;
    const pluginStyles = [
      ...Object.values(backgroundPlugins || {}).map((p) => p.styles || ''),
      ...Object.values(widgetPlugins || {}).map((p) => p.styles || ''),
    ].filter(Boolean).join('\n');

    staticStyleContent = `
      :host { all: initial; }
      :host([data-widgets-hidden="true"]) .Slot,
      :host(.widgets-hidden) .Slot,
      :host-context(body.space-widgets-hidden) .Slot {
        display: none !important;
      }
      .Widgets { width:100%; height:100%; position:relative; overflow:hidden; padding:0; text-align:center; pointer-events:none; user-select:auto; font-family:-apple-system,BlinkMacSystemFont,'PingFang SC','Segoe UI',sans-serif; }
      .background-layer { position:absolute; inset:0; background-size:cover; background-position:center; transition:background 0.3s ease, filter 0.3s ease; }
      .background-not-configured { position:absolute; inset:0; display:grid; place-items:center; color:rgba(255,255,255,.6); font-size:14px; background:#111; }
      .container { position:relative; width:100%; height:100%; }
      .Slot { position:absolute; pointer-events:none; }
      .Slot > * { margin:1rem; pointer-events:all; }
      .Slot.free > * { margin:0; }
      .Slot.topLeft { top:0; left:0; text-align:left; }
      .Slot.topCentre { top:0; left:50%; transform:translateX(-50%); text-align:center; }
      .Slot.topRight { top:0; right:0; text-align:right; }
      .Slot.middleLeft { top:50%; left:0; transform:translateY(-50%); text-align:left; }
      .Slot.middleCentre { top:50%; left:50%; transform:translate(-50%,-50%); text-align:center; }
      .Slot.middleRight { top:50%; right:0; transform:translateY(-50%); text-align:right; }
      .Slot.bottomLeft { bottom:3rem; left:0; text-align:left; }
      .Slot.bottomCentre { bottom:3rem; left:50%; transform:translateX(-50%); text-align:center; }
      .Slot.bottomRight { bottom:3rem; right:0; text-align:right; }
      .Slot.free-slot-wrap { position:absolute; }
      .Widget { position:relative; transition:color 0.15s ease; user-select:auto; display:inline-block; }
      h1, h2, h3, h4 { line-height:1; margin:0; }
      .weight-override h1, .weight-override h2, .weight-override h3, .weight-override h4 { font-weight:inherit; }
      .drag-selected { z-index:1000 !important; outline:2px dashed var(--accent, #cdf24b) !important; border-radius:4px; box-shadow:0 0 0 4px rgba(205,242,75,0.25); }
      .drag-selected > * { pointer-events:none; }
      .free-handles-wrap { position:absolute; inset:-4px; pointer-events:none; z-index:1001; }
      .free-handle { position:absolute; width:12px; height:12px; border-radius:50%; background:#fff; border:2px solid #222; pointer-events:auto; box-shadow:0 2px 6px rgba(0,0,0,0.35); }
      .free-handle.handle-scale { bottom:-6px; right:-6px; cursor:nwse-resize; }
      .free-handle.handle-rotate { top:-18px; left:50%; transform:translateX(-50%); cursor:grab; }
      .free-floating-done { position:fixed; bottom:24px; left:50%; transform:translateX(-50%); z-index:1100; display:inline-flex; align-items:center; gap:8px; padding:10px 24px; background:var(--accent, #cdf24b); color:#000; font-weight:700; font-size:14px; border:0; border-radius:999px; cursor:pointer; box-shadow:0 8px 24px rgba(0,0,0,0.4); pointer-events:auto; }
      .free-floating-done:hover { filter:brightness(1.1); transform:translateX(-50%) scale(1.03); }
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
    dashboardRoot.className = 'Widgets';
    dashboardShadow.append(dashboardRoot);

    bgLayerEl = document.createElement('div');
    bgLayerEl.className = 'background-layer';
    dashboardRoot.append(bgLayerEl);

    const containerEl = document.createElement('div');
    containerEl.className = 'container';
    dashboardRoot.append(containerEl);

    const NINE_SLOTS = [
      'topLeft', 'topCentre', 'topRight',
      'middleLeft', 'middleCentre', 'middleRight',
      'bottomLeft', 'bottomCentre', 'bottomRight',
    ];
    for (const slotName of NINE_SLOTS) {
      const slotEl = document.createElement('div');
      slotEl.className = `Slot ${slotName}`;
      containerEl.append(slotEl);
      slotContainers[slotName] = slotEl;
    }
    slotContainers.container = containerEl;

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

    for (const [id, entry] of widgetRegistry.entries()) {
      if (!incomingIds.has(id)) {
        entry.disposer?.();
        (entry.wrapperEl || entry.element).remove();
        widgetRegistry.delete(id);
      }
    }

    for (const widget of incomingWidgets) {
      const existing = widgetRegistry.get(widget.id);
      const pos = widget.displayJson?.position || 'middleCentre';

      if (!existing) {
        mountWidget(widget, pos, shadow, root, snapshot, activeWorkspaceId, updateSnapshot);
      } else {
        patchWidget(existing, widget, pos, shadow, root, snapshot, activeWorkspaceId, updateSnapshot);
      }
    }
  }

  function mountWidget(widget, pos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot) {
    const plugin = widgetPlugins[widget.key];
    if (!plugin) return;

    const container = document.createElement('div');
    const keyClass = widget.key.replace('widget/', '');
    container.dataset.widgetId = widget.id;

    applyWidgetDisplayStyles(container, widget.displayJson || {}, keyClass, false);

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

    let wrapperEl = null;
    if (pos === 'free') {
      wrapperEl = document.createElement('div');
      wrapperEl.className = 'Slot free free-slot-wrap';
      const disp = widget.displayJson || {};
      wrapperEl.style.left = `${disp.xPercent ?? 50}%`;
      wrapperEl.style.top = `${disp.yPercent ?? 50}%`;
      wrapperEl.style.transform = `translate(-50%, -50%) scale(${disp.scale ?? 1}) rotate(${disp.rotation ?? 0}deg)`;
      wrapperEl.dataset.widgetId = widget.id;
      wrapperEl.append(container);
      slotContainers.container?.append(wrapperEl);
    } else {
      const targetSlot = slotContainers[pos] || slotContainers.middleCentre;
      targetSlot?.append(container);
    }

    widgetRegistry.set(widget.id, {
      element: container,
      wrapperEl,
      widget,
      pos,
      keyClass,
      disposer,
    });
  }

  function patchWidget(entry, nextWidget, nextPos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot) {
    const prevWidget = entry.widget;
    entry.widget = nextWidget;

    if (entry.pos !== nextPos) {
      entry.disposer?.();
      (entry.wrapperEl || entry.element).remove();
      widgetRegistry.delete(nextWidget.id);
      mountWidget(nextWidget, nextPos, shadowRoot, root, snapshot, activeWorkspaceId, updateSnapshot);
      return;
    }

    const isEditing = currentEditingWidgetId === nextWidget.id;
    applyWidgetDisplayStyles(entry.element, nextWidget.displayJson || {}, entry.keyClass, isEditing);

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

  function applyWidgetDisplayStyles(container, disp, keyClass, isEditing = false) {
    container.style.color = disp.useAccentColor ? 'var(--accent, #cdf24b)' : (disp.colour || '');
    if (disp.fontFamily) container.style.fontFamily = disp.fontFamily;
    container.style.fontSize = disp.fontSize ? `${disp.fontSize}px` : '';
    container.style.fontWeight = disp.fontWeight ? String(disp.fontWeight) : '';
    container.style.fontStyle = disp.fontStyle || '';
    container.style.textDecoration = disp.textDecoration || '';

    const origin = transformOriginMap[disp.position || 'middleCentre'] || 'center center';
    container.style.transformOrigin = origin;

    if (!isEditing && disp.position !== 'free') {
      const s = disp.scale ?? 1;
      const r = disp.rotation ?? 0;
      container.style.transform = (s !== 1 || r !== 0) ? `scale(${s}) rotate(${r}deg)` : '';
    }

    if (disp.textOutline) {
      const outlineColor = disp.textOutlineColor || '#000000';
      if (disp.textOutlineStyle === 'advanced') {
        const outlineSize = Number(disp.textOutlineSize) || 1;
        container.style.webkitTextStroke = `${outlineSize * 2}px ${outlineColor}`;
        container.style.textShadow = '';
      } else {
        container.style.webkitTextStroke = '';
        container.style.textShadow = `-1px -1px 0 ${outlineColor}, 1px -1px 0 ${outlineColor}, -1px 1px 0 ${outlineColor}, 1px 1px 0 ${outlineColor}`;
      }
    } else {
      container.style.webkitTextStroke = '';
      container.style.textShadow = '';
    }

    container.className = `Widget widget-${keyClass || ''} ${disp.fontWeight ? 'weight-override' : ''}`;
    if (disp.customClass && /^[a-zA-Z0-9_-]+$/.test(disp.customClass)) {
      container.classList.add(disp.customClass);
    }
  }

  function setEditingWidget(widgetId, snapshot, activeWorkspaceId, updateSnapshot) {
    currentEditingWidgetId = widgetId;
    const { shadow, root } = ensureShadowShell();
    if (!shadow || !root) return;

    // Remove existing edit handles and floating button
    shadow.querySelectorAll('.free-handles-wrap, .free-floating-done').forEach((el) => el.remove());
    shadow.querySelectorAll('.drag-selected').forEach((el) => el.classList.remove('drag-selected'));

    if (!widgetId) return;

    const entry = widgetRegistry.get(widgetId);
    if (!entry || entry.pos !== 'free' || !entry.wrapperEl) return;

    const targetWrapper = entry.wrapperEl;
    targetWrapper.classList.add('drag-selected');

    // Create handles
    const handlesWrap = document.createElement('div');
    handlesWrap.className = 'free-handles-wrap';
    const scaleHandle = document.createElement('div');
    scaleHandle.className = 'free-handle handle-scale';
    const rotHandle = document.createElement('div');
    rotHandle.className = 'free-handle handle-rotate';
    handlesWrap.append(scaleHandle, rotHandle);
    targetWrapper.append(handlesWrap);

    // Floating Done Button
    const doneBtn = document.createElement('button');
    doneBtn.className = 'free-floating-done';
    doneBtn.type = 'button';
    doneBtn.innerHTML = `<span>✓</span><span>${t('doneEditingPosition', '完成调整')}</span>`;
    doneBtn.onclick = () => {
      setEditingWidget(null);
    };
    root.append(doneBtn);

    attachEditingGestures(targetWrapper, scaleHandle, rotHandle, entry.widget, snapshot, activeWorkspaceId, updateSnapshot);
  }

  function attachEditingGestures(element, scaleHandle, rotHandle, widget, snapshot, activeWorkspaceId, updateSnapshot) {
    let isInteracting = false;
    let mode = 'idle';
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

      element.style.left = `${finalX}%`;
      element.style.top = `${finalY}%`;
      element.style.transform = `translate(-50%, -50%) scale(${finalScale}) rotate(${finalRot}deg)`;

      mode = 'idle';
      curDx = 0;
      curDy = 0;
      initXPercent = finalX;
      initYPercent = finalY;
      initScale = finalScale;
      initRot = finalRot;

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
      } catch (err) {}
    };

    element.onpointercancel = () => {
      isInteracting = false;
      mode = 'idle';
      element.style.willChange = 'auto';
    };
  }

  return {
    render,
    setEditingWidget,
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
