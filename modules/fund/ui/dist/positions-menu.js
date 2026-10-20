(() => {
// 持仓操作浮层的唯一生命周期 owner；不读取账本、不发请求、不创建 timer。
const menus = () => [...document.querySelectorAll(".pos-more")];
const shown = () => menus().find(d => d.open);
const popup = d => d.querySelector(".pos-action-menu");
const trigger = d => d.querySelector("summary");
const items = d => [...d.querySelectorAll('[role="menuitem"]')];
let anchor = null;
function close(returnFocus = false) {
  const d = shown();
  if (!d) return;
  if (popup(d).matches(":popover-open")) popup(d).hidePopover();
  d.open = false;
  trigger(d).setAttribute("aria-expanded", "false");
  anchor = null;
  if (returnFocus) trigger(d).focus({ preventScroll: true });
}
function focusItem(d, index) {
  const buttons = items(d), button = buttons[index];
  if (!button) return;
  buttons.forEach(b => { b.tabIndex = b === button ? 0 : -1; });
  button.focus({ preventScroll: true });
  const menu = popup(d), m = menu.getBoundingClientRect(), b = button.getBoundingClientRect();
  const scale = m.width / menu.offsetWidth;
  if (b.bottom > m.bottom - 8) menu.scrollTop += (b.bottom - m.bottom + 8) / scale;
  else if (b.top < m.top + 8) menu.scrollTop += (b.top - m.top - 8) / scale;
}
function open(d, index = null) {
  close();
  d.open = true;
  trigger(d).setAttribute("aria-expanded", "true");
  const menu = popup(d);
  menu.showPopover();
  const scale = menu.getBoundingClientRect().width / menu.offsetWidth;
  const r = trigger(d).getBoundingClientRect();
  const below = window.innerHeight - r.bottom - 12, above = r.top - 12;
  const placeBelow = below >= menu.scrollHeight * scale || below >= above;
  menu.style.maxHeight = Math.max(32, placeBelow ? below : above) / scale + "px";
  menu.style.maxWidth = (window.innerWidth - 16) / scale + "px";
  const m = menu.getBoundingClientRect();
  anchor = { x: r.x, y: r.y };
  const left = Math.max(8, Math.min(r.right - m.width, window.innerWidth - m.width - 8));
  const y = placeBelow ? r.bottom + 4 : r.top - m.height - 4;
  menu.style.left = left / scale + "px";
  menu.style.top = Math.max(8, Math.min(y, window.innerHeight - m.height - 8)) / scale + "px";
  menu.scrollTop = 0;
  if (index != null) focusItem(d, index);
}
function capture() {
  const d = shown(), focused = document.activeElement?.closest(".pos-more");
  return { openId: d?.dataset.posMenuId, focusId: focused?.dataset.posMenuId,
    action: document.activeElement?.dataset.posAct };
}
function bind(saved = {}) {
  menus().forEach(d => {
    const button = trigger(d), menu = popup(d);
    items(d).forEach((b, i) => { b.tabIndex = i === 0 ? 0 : -1; });
    button.addEventListener("click", e => {
      e.preventDefault();
      if (d.open) close(true); else open(d, 0);
    });
    button.addEventListener("keydown", e => {
      if (!["ArrowDown", "ArrowUp"].includes(e.key)) return;
      e.preventDefault();
      open(d, e.key === "ArrowUp" ? items(d).length - 1 : 0);
    });
    menu.addEventListener("beforetoggle", e => {
      if (e.newState === "closed") {
        d.open = false;
        button.setAttribute("aria-expanded", "false");
      }
    });
    menu.addEventListener("keydown", e => {
      const buttons = items(d), index = buttons.indexOf(document.activeElement);
      if (e.key === "Tab") { close(true); return; }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      focusItem(d, e.key === "Home" ? 0 : e.key === "End" ? buttons.length - 1
        : (index + (e.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length);
    });
    if (d.dataset.posMenuId === saved.openId) {
      const index = saved.focusId === saved.openId && saved.action
        ? items(d).findIndex(b => b.dataset.posAct === saved.action) : null;
      open(d, index === -1 ? 0 : index);
    }
    if (d.dataset.posMenuId === saved.focusId && !saved.action) button.focus({ preventScroll: true });
  });
}
document.addEventListener("keydown", e => {
  if (e.key === "Escape" && shown()) { e.preventDefault(); e.stopPropagation(); close(true); }
}, true);
window.addEventListener("resize", () => close(true));
document.addEventListener("scroll", e => {
  const d = shown();
  if (!d || !anchor || popup(d).contains(e.target)) return;
  const r = trigger(d).getBoundingClientRect();
  if (Math.abs(r.x - anchor.x) > 1 || Math.abs(r.y - anchor.y) > 1) close(popup(d).contains(document.activeElement));
}, { capture: true, passive: true });
globalThis.Fund.positionMenus = { capture, bind, close };
})();
