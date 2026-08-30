/**
 * Space Shadow DOM Dashboard controller (<280 lines).
 * Handles: background rendering with backdrop filters, nine-grid slots,
 * full typography/display styles, and free-drag/scale/rotate gestures with zero IPC during move.
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

  function buildShadow() {
    const host = $('dashboard-host');
    if (dashboardShadow && dashboardRoot && host.shadowRoot === dashboardShadow) {
      dashboardRoot.replaceChildren();
      return { shadow: dashboardShadow, root: dashboardRoot };
    }
    host.replaceChildren();
    dashboardShadow = host.attachShadow({ mode: 'open' });

    const pluginStyles = [
      ...Object.values(backgroundPlugins || {}).map((p) => p.styles || ''),
      ...Object.values(widgetPlugins || {}).map((p) => p.styles || ''),
    ].filter(Boolean).join('\n');

    const style = document.createElement('style');
    style.textContent = `
      :host { all: initial; }
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
    dashboardShadow.append(style);
    dashboardRoot = document.createElement('div');
    dashboardRoot.className = 'dashboard';
    dashboardShadow.append(dashboardRoot);
    return { shadow: dashboardShadow, root: dashboardRoot };
  }

  function render(snapshot, activeWorkspaceId, updateSnapshot) {
    if (!snapshot) return;
    const { shadow, root } = buildShadow();

    // 1. Render Background with Backdrop Filters (blur, brightness, night mode)
    const bgData = snapshot.backgroundJson || {};
    const bgKey = bgData.key || 'background/colour';
    const bgPlugin = backgroundPlugins[bgKey] || backgroundPlugins['background/colour'];
    const bgDisplay = bgData.display || bgData.data || {};

    if (bgPlugin) {
      const bgContainer = document.createElement('div');
      bgContainer.className = 'background-layer';

      // Check night mode dimming
      let isNight = false;
      if (bgDisplay.nightDim) {
        const h = new Date().getHours();
        isNight = h >= 20 || h < 6;
      }

      const blurPx = Number(bgDisplay.blur) || 0;
      let bright = bgDisplay.brightness ?? (isNight ? 0.6 : 1);
      if (blurPx > 0 || bright !== 1) {
        bgContainer.style.filter = `blur(${blurPx}px) brightness(${bright})`;
      }

      bgPlugin.render(bgContainer, bgDisplay, { t, lang: selectedLanguage });
      root.append(bgContainer);
    }

    // 2. Group Widgets by position
    const byPosition = {};
    const freeWidgets = [];
    for (const widget of snapshot.widgets || []) {
      if (!widget.enabled) continue;
      const pos = widget.displayJson?.position || 'middleCentre';
      if (pos === 'free') {
        freeWidgets.push(widget);
      } else {
        (byPosition[pos] = byPosition[pos] || []).push(widget);
      }
    }

    // 3. Render Nine-grid slots
    for (const [pos, widgets] of Object.entries(byPosition)) {
      const slot = document.createElement('div');
      slot.className = `slot ${pos}`;
      for (const widget of widgets.sort((a, b) => a.order - b.order)) {
        const el = createWidgetEl(widget, shadow, snapshot, activeWorkspaceId, updateSnapshot);
        if (el) slot.append(el);
      }
      root.append(slot);
    }

    // 4. Render Free-positioned Widgets
    for (const widget of freeWidgets) {
      const freeWrapper = document.createElement('div');
      freeWrapper.className = 'slot free-widget';
      const disp = widget.displayJson || {};
      const x = disp.xPercent ?? disp.x ?? 50;
      const y = disp.yPercent ?? disp.y ?? 50;
      const scale = disp.scale ?? 1;
      const rot = disp.rotation ?? 0;
      freeWrapper.style.left = `${x}%`;
      freeWrapper.style.top = `${y}%`;
      freeWrapper.style.transform = `translate(-50%, -50%) scale(${scale}) rotate(${rot}deg)`;
      freeWrapper.dataset.widgetId = widget.id;

      // Handles for scale and rotate
      const scaleHandle = document.createElement('div');
      scaleHandle.className = 'free-handle handle-scale';
      const rotHandle = document.createElement('div');
      rotHandle.className = 'free-handle handle-rotate';
      freeWrapper.append(scaleHandle, rotHandle);

      attachFreeGestures(freeWrapper, scaleHandle, rotHandle, widget, snapshot, activeWorkspaceId, updateSnapshot);
      const el = createWidgetEl(widget, shadow, snapshot, activeWorkspaceId, updateSnapshot);
      if (el) freeWrapper.append(el);
      root.append(freeWrapper);
    }
  }

  function createWidgetEl(widget, shadowRoot, snapshot, activeWorkspaceId, updateSnapshot) {
    const plugin = widgetPlugins[widget.key];
    if (!plugin) return null;
    const container = document.createElement('div');
    const keyClass = widget.key.replace('widget/', '');
    container.className = `widget-container widget-${keyClass}`;
    container.dataset.widgetId = widget.id;

    // Apply Display & Typography configuration
    const disp = widget.displayJson || {};
    if (disp.fontSize) container.style.fontSize = `${disp.fontSize}px`;
    if (disp.colour && !disp.useAccentColor) container.style.color = disp.colour;
    if (disp.useAccentColor) container.style.color = 'var(--accent, #cdf24b)';
    if (disp.fontWeight) container.style.fontWeight = String(disp.fontWeight);
    if (disp.fontStyle) container.style.fontStyle = disp.fontStyle;
    if (disp.textDecoration) container.style.textDecoration = disp.textDecoration;

    // Custom CSS class (validated token)
    if (disp.customClass && /^[a-zA-Z0-9_-]+$/.test(disp.customClass)) {
      container.classList.add(disp.customClass);
    }

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
    return container;
  }

  function attachFreeGestures(element, scaleHandle, rotHandle, widget, snapshot, activeWorkspaceId, updateSnapshot) {
    let isDragging = false;
    let isScaling = false;
    let isRotating = false;
    let startX = 0;
    let startY = 0;
    let initXPercent = widget.displayJson?.xPercent ?? 50;
    let initYPercent = widget.displayJson?.yPercent ?? 50;
    let initScale = widget.displayJson?.scale ?? 1;
    let initRot = widget.displayJson?.rotation ?? 0;

    element.onpointerdown = (e) => {
      if (['INPUT', 'TEXTAREA', 'BUTTON', 'A'].includes(e.target.tagName)) return;
      if (e.target === scaleHandle || e.target === rotHandle) return;
      isDragging = true;
      startX = e.clientX;
      startY = e.clientY;
      element.setPointerCapture(e.pointerId);
      e.stopPropagation();
    };

    scaleHandle.onpointerdown = (e) => {
      isScaling = true;
      startX = e.clientX;
      scaleHandle.setPointerCapture(e.pointerId);
      e.stopPropagation();
    };

    rotHandle.onpointerdown = (e) => {
      isRotating = true;
      startX = e.clientX;
      rotHandle.setPointerCapture(e.pointerId);
      e.stopPropagation();
    };

    element.onpointermove = (e) => {
      const host = $('dashboard-host');
      if (isDragging) {
        const dx = ((e.clientX - startX) / host.offsetWidth) * 100;
        const dy = ((e.clientY - startY) / host.offsetHeight) * 100;
        element.style.left = `${Math.max(0, Math.min(100, initXPercent + dx))}%`;
        element.style.top = `${Math.max(0, Math.min(100, initYPercent + dy))}%`;
      } else if (isScaling) {
        const ds = (e.clientX - startX) * 0.01;
        const nextScale = Math.max(0.2, Math.min(3, initScale + ds));
        element.style.transform = `translate(-50%, -50%) scale(${nextScale}) rotate(${initRot}deg)`;
      } else if (isRotating) {
        const dr = (e.clientX - startX) * 1.5;
        const nextRot = Math.round(initRot + dr);
        element.style.transform = `translate(-50%, -50%) scale(${initScale}) rotate(${nextRot}deg)`;
      }
    };

    element.onpointerup = async (e) => {
      if (!isDragging && !isScaling && !isRotating) return;
      const wasDrag = isDragging;
      const wasScale = isScaling;
      const wasRotate = isRotating;
      isDragging = false;
      isScaling = false;
      isRotating = false;

      const host = $('dashboard-host');
      const dx = ((e.clientX - startX) / host.offsetWidth) * 100;
      const dy = ((e.clientY - startY) / host.offsetHeight) * 100;
      const finalX = wasDrag ? Math.round(Math.max(0, Math.min(100, initXPercent + dx)) * 10) / 10 : initXPercent;
      const finalY = wasDrag ? Math.round(Math.max(0, Math.min(100, initYPercent + dy)) * 10) / 10 : initYPercent;
      const finalScale = wasScale ? Math.round(Math.max(0.2, Math.min(3, initScale + (e.clientX - startX) * 0.01)) * 10) / 10 : initScale;
      const finalRot = wasRotate ? Math.round(initRot + (e.clientX - startX) * 1.5) : initRot;

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
      isDragging = false;
      isScaling = false;
      isRotating = false;
      render(snapshot, activeWorkspaceId, updateSnapshot);
    };
  }

  return { render };
}
