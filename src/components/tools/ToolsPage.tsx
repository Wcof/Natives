'use client';

import { useState } from 'react';
import { Camera, Download, ExternalLink, PackageCheck, RefreshCw } from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { t as tr, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';

/**
 * 工具页（Hub 面）— 只呈现真实能力：
 * - 截图快递：说明全局监听行为（浮卡由 ShellLayout 常驻的 ScreenshotCard 负责）
 * - 发布向导：入口按钮，打开 components/release 的真实向导（open-release-wizard 事件）
 * - 更新检查：手动触发 update.check 并内联展示结果
 * 此前这里是三个假组件（死按钮浮卡 / 永远 loading 的假清单 / 重复挂载的更新通知）。
 */

interface UpdateInfo {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  releaseUrl: string | null;
  publishedAt: string | null;
  body: string | null;
  sourceConfigured: boolean;
  message: string;
}

export default function ToolsPage() {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  return (
    <div style={{ height: '100%', overflow: 'auto', padding: SPACING.xl }}>
      <div style={{ display: 'grid', gap: SPACING.md, maxWidth: 720 }}>
        <ScreenshotSection t={t} />
        <ReleaseSection t={t} />
        <UpdateSection t={t} />
      </div>
    </div>
  );
}

function Card({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <section
      style={{
        border: '1px solid var(--border)',
        borderRadius: BORDER_RADIUS.lg,
        background: 'var(--surface)',
        padding: SPACING.lg,
      }}
      aria-label={title}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
        <span style={{ color: 'var(--primary)', display: 'inline-flex' }}>{icon}</span>
        <h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)' }}>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function ScreenshotSection({ t }: { t: (key: string) => string }) {
  return (
    <Card icon={<Camera size={16} />} title={t('screenshot.title')}>
      <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.6 }}>
        {t('tools.screenshotDesc')}
      </p>
    </Card>
  );
}

function ReleaseSection({ t }: { t: (key: string) => string }) {
  return (
    <Card icon={<PackageCheck size={16} />} title={t('tools.releaseWizard')}>
      <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.6, marginBottom: SPACING.md }}>
        {t('tools.releaseDesc')}
      </p>
      <button
        type="button"
        className="btn btn-primary btn-sm"
        onClick={() => window.dispatchEvent(new CustomEvent('open-release-wizard'))}
      >
        {t('tools.openReleaseWizard')}
      </button>
    </Card>
  );
}

function UpdateSection({ t }: { t: (key: string) => string }) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleCheck = async () => {
    const api = window.nativesAPI;
    if (!api?.update?.check) {
      setError(classifyError(new Error('Update API unavailable')).userMessage);
      return;
    }
    setChecking(true);
    setError(null);
    try {
      setResult((await api.update.check()) as UpdateInfo);
    } catch (e) {
      setResult(null);
      setError(classifyError(e).userMessage);
    } finally {
      setChecking(false);
    }
  };

  return (
    <Card icon={<Download size={16} />} title={t('update.title')}>
      <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.6, marginBottom: SPACING.md }}>
        {t('tools.updateDesc')}
      </p>
      <button
        type="button"
        className="btn btn-sm"
        onClick={handleCheck}
        disabled={checking}
        style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
      >
        <RefreshCw size={13} className={checking ? 'animate-spin' : undefined} />
        {checking ? t('update.checking') : t('update.checkNow')}
      </button>

      {error && (
        <p style={{ marginTop: SPACING.sm, fontSize: FONT_SIZE.sm, color: 'var(--danger)' }}>{error}</p>
      )}

      {result && !error && (
        <div style={{ marginTop: SPACING.md, fontSize: FONT_SIZE.sm, color: 'var(--text)' }}>
          <div style={{ color: 'var(--text-secondary)', marginBottom: 4 }}>
            {t('tools.currentVersion')}: <span style={{ fontFamily: 'var(--font-mono)' }}>v{result.currentVersion}</span>
          </div>
          {!result.sourceConfigured ? (
            <p style={{ color: 'var(--text-disabled)' }}>{result.message || t('update.noUpdates')}</p>
          ) : result.updateAvailable && result.latestVersion ? (
            <div>
              <p style={{ fontWeight: 600, marginBottom: 4 }}>
                {t('update.newVersionAvailable').replace('{version}', result.latestVersion)}
              </p>
              {result.body && (
                <p style={{
                  color: 'var(--text-secondary)', marginBottom: SPACING.sm,
                  maxHeight: 72, overflow: 'hidden', whiteSpace: 'pre-wrap',
                }}>
                  {result.body.slice(0, 300)}{result.body.length > 300 ? '…' : ''}
                </p>
              )}
              {result.releaseUrl && (
                <a
                  href={result.releaseUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="btn btn-primary btn-sm"
                  style={{ textDecoration: 'none', display: 'inline-flex', alignItems: 'center', gap: 4 }}
                >
                  <ExternalLink size={12} /> {t('update.download')}
                </a>
              )}
            </div>
          ) : (
            <p style={{ color: 'var(--diff-add)' }}>{t('update.upToDate')}</p>
          )}
        </div>
      )}
    </Card>
  );
}
