export function attachPositionGestures({ element, scaleHandle, rotHandle, display,
  commitDisplay, reportError }) {
  let mode = '', pending = false, disposed = false, raf = null, pointerId;
  let startX = 0, startY = 0;
  let confirmed = {
    xPercent: display.xPercent ?? 50, yPercent: display.yPercent ?? 50,
    scale: display.scale ?? 1, rotation: display.rotation ?? 0,
  };
  let draft = { ...confirmed };
  const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
  const paint = () => {
    element.style.left = `${draft.xPercent}%`;
    element.style.top = `${draft.yPercent}%`;
    element.style.transform = `translate(-50%, -50%) scale(${draft.scale}) rotate(${draft.rotation}deg)`;
    raf = null;
  };
  const endPointer = () => {
    mode = '';
    if (pointerId !== undefined && element.hasPointerCapture(pointerId)) element.releasePointerCapture(pointerId);
    pointerId = undefined;
    if (raf !== null) cancelAnimationFrame(raf);
    raf = null;
    element.style.willChange = 'auto';
  };
  const cancel = () => { endPointer(); draft = { ...confirmed }; paint(); };
  const start = (next, event) => {
    if (event.button !== 0 || pending || disposed) return;
    mode = next;
    draft = { ...confirmed };
    startX = event.clientX;
    startY = event.clientY;
    pointerId = event.pointerId;
    element.setPointerCapture(pointerId);
    element.focus({ preventScroll: true });
    element.style.willChange = 'transform';
    event.preventDefault();
    event.stopPropagation();
  };
  async function commit() {
    endPointer();
    draft.xPercent = Math.round(draft.xPercent * 10) / 10;
    draft.yPercent = Math.round(draft.yPercent * 10) / 10;
    draft.scale = Math.round(draft.scale * 10) / 10;
    if (JSON.stringify(draft) === JSON.stringify(confirmed)) { cancel(); return; }
    const next = { ...draft };
    pending = true;
    paint();
    try {
      await commitDisplay(next);
      confirmed = next;
    } catch (error) {
      if (!disposed) cancel();
      reportError(error);
    } finally { pending = false; }
  }
  element.tabIndex = 0;
  element.onpointerdown = event => start('drag', event);
  scaleHandle.onpointerdown = event => start('scale', event);
  rotHandle.onpointerdown = event => start('rotate', event);
  element.onpointermove = event => {
    if (!mode) return;
    const dx = event.clientX - startX, dy = event.clientY - startY;
    const host = element.getRootNode().host;
    if (mode === 'drag') {
      draft.xPercent = clamp(confirmed.xPercent + dx / (host?.offsetWidth || innerWidth) * 100, 0, 100);
      draft.yPercent = clamp(confirmed.yPercent + dy / (host?.offsetHeight || innerHeight) * 100, 0, 100);
    }
    if (mode === 'scale') draft.scale = clamp(confirmed.scale + dx * .01, .2, 3);
    if (mode === 'rotate') draft.rotation = Math.round(confirmed.rotation + dx * 1.5);
    if (raf === null) raf = requestAnimationFrame(paint);
  };
  element.onpointerup = () => { if (mode) void commit(); };
  element.onpointercancel = cancel;
  const keydown = event => {
    if (event.key === 'Escape' && mode) {
      event.preventDefault();
      event.stopImmediatePropagation();
      cancel();
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey || pending || !event.composedPath().includes(element)) return;
    const delta = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!delta) return;
    event.preventDefault();
    draft = { ...confirmed, xPercent: clamp(confirmed.xPercent + delta[0], 0, 100), yPercent: clamp(confirmed.yPercent + delta[1], 0, 100) };
    void commit();
  };
  const keyOptions = { capture: true };
  document.addEventListener('keydown', keydown, keyOptions);
  return () => {
    disposed = true;
    if (pending) endPointer();
    else cancel();
    element.onpointerdown = element.onpointermove = element.onpointerup = element.onpointercancel = null;
    scaleHandle.onpointerdown = rotHandle.onpointerdown = null;
    element.removeAttribute('tabindex');
    document.removeEventListener('keydown', keydown, keyOptions);
  };
}
