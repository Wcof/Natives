

import { fetchDedup } from '../plugins-cache.js';

export const ipInfoWidget = {
  key: 'widget/ipInfo',
  name: 'IP Info',
  defaultData: {},
  render(container, data = {}, display = {}, { t = (k, f) => f || k } = {}) {
    container.className = 'Widget IpInfo';
    container.textContent = t('ipDetecting', 'IP: 检测中…');
    let disposed = false;

    fetchDedup(
      'ip_info',
      async () => {
        const res = await fetch('https://api.ipify.org?format=json');
        if (res.ok === false) throw new Error('IP service unreachable');
        const json = await res.json();
        return json?.ip ? { ip: json.ip } : null;
      },
      30 * 60 * 1000,
    )
      .then((cached) => {
        if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;
        if (cached?.ip) {
          container.textContent = `IP: ${cached.ip}`;
        } else {
          container.textContent = t('ipUnavailable', 'IP: 未能获取');
        }
      })
      .catch((err) => {
        if (disposed || (container.isConnected !== undefined && !container.isConnected)) return;
        container.textContent = `${t('ipFetchFailed', 'IP 获取失败')}: ${err.message || 'Error'}`;
      });

    return () => {
      disposed = true;
    };
  },
  renderSettings(container, data = {}, onChange = () => {}, { t = (k, f) => f || k } = {}) {
    container.replaceChildren();
    const notice = document.createElement('div');
    notice.className = 'inspector-notice';
    notice.textContent = t('ipInfoNotice', '自动安全检测并展示当前公网 IP 地址。');
    container.append(notice);
  },
  styles: `
    .IpInfo { text-align: center; }
  `,
};
