// Fund App UI (build-time placeholder — Phase F0+).
//
// Present at build time, imported only when the fund app is installed
// (ADR-0025 D9). Until Phase F0 the Fund Native Runtime (fund-host) does
// not exist yet, so this module renders the installed-but-empty state
// without opening any host port.

export function mountApp(ctx) {
  const { app, stage, t } = ctx;
  const box = document.createElement('div');
  box.className = 'app-card';
  box.innerHTML = `<h3>${escapeHtml(app.name)}</h3>`;
  const p = document.createElement('p');
  p.textContent = t('fundEmpty', '基金应用已安装，暂无资产。');
  box.append(p);
  stage.replaceChildren(box);
  return () => stage.replaceChildren();
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
