window.__ModuleLoader__.load({
  id: '@natives/dsh-space',
  factory(require) {
    const React = require('react');
    const h = React.createElement;
    const NS = 'nativesSpace';
    const PANEL_ID = 'natives-space';
    const ROUTE = '/natives/space/url';
    const COPY = {
      zh: {
        'panel.title': '空间',
        'panel.description': '在主面板中打开 Natives 空间。',
        'panel.opening': '正在连接 Natives…',
        'panel.opened': '面板已加载。',
        'panel.retry': '重新打开',
        'error.core-missing': '未找到 Natives 产品源：请先安装 Natives 桌面端。',
        'error.schema-too-new': '本机数据由更新版本写入，请先升级插件。',
        'error.unknown-target': '未知模块。',
        'error.launcher-failed': 'Natives 启动失败，请重试。',
        'error.open-failed': '宿主浏览器标签未能打开。',
        'error.network': '无法连接 Natives 运行时。',
        'error.unknown': '发生未知错误。',
      },
      en: {
        'panel.title': 'Space',
        'panel.description': 'Opens the Natives space page in the main panel.',
        'panel.opening': 'Connecting to Natives…',
        'panel.opened': 'Panel loaded.',
        'panel.retry': 'Reopen',
        'error.core-missing': 'No Natives product source found. Install the Natives desktop app first.',
        'error.schema-too-new': 'Local data was written by a newer version. Upgrade the plugin first.',
        'error.unknown-target': 'Unknown module.',
        'error.launcher-failed': 'Natives failed to start. Try again.',
        'error.open-failed': 'The host browser tab could not be opened.',
        'error.network': 'Cannot reach the Natives runtime.',
        'error.unknown': 'Something went wrong.',
      },
    };

    function SpaceIcon(props) {
      const size = props && props.size ? props.size : 16;
      return h(
        'svg',
        {
          viewBox: '0 0 16 16',
          width: size,
          height: size,
          fill: 'none',
          stroke: 'currentColor',
          strokeWidth: props && props.active ? 1.8 : 1.4,
          strokeLinejoin: 'round',
          'aria-hidden': true,
          style: { display: 'block' },
        },
        h('rect', { x: 2, y: 2.5, width: 12, height: 11, rx: 2 }),
        h('path', { d: 'M2 6h12M6 6v7.5' })
      );
    }

    const currentTheme = () => {
      try {
        return getComputedStyle(document.documentElement).colorScheme.includes('dark') ? 'volt' : 'archive';
      } catch {
        return 'archive';
      }
    };

    function createPanel(ctx, t) {
      return function SpacePanel() {
        const [state, setState] = React.useState({ kind: 'opening' });
        const requested = React.useRef(false);

        const open = React.useCallback(async () => {
          setState({ kind: 'opening' });
          let payload;
          try {
            const response = await fetch(`${ROUTE}?theme=${currentTheme()}`, {
              credentials: 'same-origin',
              cache: 'no-store',
            });
            payload = await response.json().catch(() => ({}));
            if (!response.ok) {
              setState({ kind: 'error', code: payload.code ?? 'unknown', detail: payload.message });
              return;
            }
          } catch (error) {
            setState({ kind: 'error', code: 'network', detail: String((error && error.message) || error) });
            return;
          }
          setState({ kind: 'ready', url: payload.url });
        }, []);

        React.useEffect(() => {
          if (requested.current) return;
          requested.current = true;
          open();
        }, [open]);

        if (state.kind === 'ready') {
          return h('iframe', {
            title: t('panel.title'),
            src: state.url,
            style: { border: 0, width: '100%', height: '100%', display: 'block', background: 'transparent' },
          });
        }

        const key = state.kind === 'error' ? `error.${state.code}` : `panel.${state.kind}`;
        const message = t(Object.prototype.hasOwnProperty.call(COPY.zh, key) ? key : 'error.unknown');

        return h(
          'div',
          { style: { padding: 24, display: 'grid', gap: 12, maxWidth: 420, color: 'inherit' } },
          h('h2', { style: { margin: 0, fontSize: 16, fontWeight: 600 } }, t('panel.title')),
          h('p', { style: { margin: 0, fontSize: 13, lineHeight: 1.6, opacity: 0.72 } }, t('panel.description')),
          h('p', { style: { margin: 0, fontSize: 13, lineHeight: 1.6 } }, message),
          h(
            'button',
            {
              type: 'button',
              onClick: open,
              style: {
                justifySelf: 'start',
                padding: '6px 14px',
                borderRadius: 8,
                border: '1px solid currentColor',
                background: 'transparent',
                color: 'inherit',
                font: 'inherit',
                cursor: 'pointer',
              },
            },
            t('panel.retry')
          )
        );
      };
    }

    return {
      inject: ['slots', 'locale'],
      apply(ctx) {
        ctx.effect(() => ctx.locale.register(NS, COPY), 'natives-space:copy');
        const t = ctx.locale.bind(NS);
        const Panel = createPanel(ctx, t);
        ctx.slots.inject('sidebar.panellist', () =>
          ctx.slots.register(
            { name: 'sidebar.panellist', id: PANEL_ID, order: 20, label: () => t('panel.title'), locale: NS },
            SpaceIcon
          )
        );
        ctx.slots.inject('main', () =>
          ctx.slots.register({ name: 'main', key: PANEL_ID, locale: NS }, Panel)
        );
      },
    };
  },
});
