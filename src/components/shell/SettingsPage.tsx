'use client';

import { useState, useEffect } from 'react';
import { THEMES } from '@/lib/theme-engine';
import { useTheme } from '@/context/ThemeContext';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { Trash2, Shield, Settings, Terminal as TermIcon, Check, HelpCircle, Loader2 } from 'lucide-react';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import ProviderDetail from '@/components/settings/ProviderDetail';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';

export default function SettingsPage({ activeTab = 'theme' }: { activeTab?: 'theme' | 'env' | 'plugins' | 'executor' | 'providers' }) {
  const { toast: globalToast } = useToast();
  const [locale, setLocaleState] = useState<Locale>('zh');
  const { themeId, setTheme } = useTheme();

  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);

  // ── Plugins State ──
  const [pluginStatus, setPluginStatus] = useState<Record<string, { installed: boolean; version: string | null; loading: boolean }>>({
    rtk: { installed: false, version: null, loading: true },
    codegraph: { installed: false, version: null, loading: true },
    ccusage: { installed: false, version: null, loading: true },
  });
  const [activeInstallPlugin, setActiveInstallPlugin] = useState<string | null>(null);
  const [installLogs, setInstallLogs] = useState<string[]>([]);

  useEffect(() => { if (activeTab === 'providers') loadProviders(); }, [activeTab]);

  useEffect(() => {
    (async () => {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const sl = await api.getLocale().catch(() => null);
        if (sl) setLocaleState(sl as Locale);
      } catch { /* noop */ }
    })();
  }, []);

  // ── Detect Plugins ──
  useEffect(() => {
    if (activeTab === 'plugins') {
      detectPlugins();
    }
  }, [activeTab]);

  // ── Listen to Plugins Install/Uninstall Logs ──
  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.plugins) return;

    const unSubLog = api.plugins.onInstallLog((log) => {
      setInstallLogs((prev) => [...prev, log]);
    });

    const unSubComplete = api.plugins.onInstallComplete((payload) => {
      if (payload.success) {
        globalToast(locale === 'zh' ? '插件安装成功' : 'Plugin installed successfully', 'success');
      } else {
        globalToast(payload.error || 'Failed to install plugin', 'error');
      }
      setActiveInstallPlugin(null);
      detectPlugins();
    });

    const unSubUninstallComplete = api.plugins.onUninstallComplete((payload) => {
      if (payload.success) {
        globalToast(locale === 'zh' ? '插件卸载成功' : 'Plugin uninstalled successfully', 'success');
      } else {
        globalToast(payload.error || 'Failed to uninstall plugin', 'error');
      }
      setActiveInstallPlugin(null);
      detectPlugins();
    });

    return () => {
      unSubLog();
      unSubComplete();
      unSubUninstallComplete();
    };
  }, [locale, globalToast]);

  async function detectPlugins() {
    const api = window.nativesAPI;
    if (!api?.plugins?.detect) return;

    for (const name of ['rtk', 'codegraph', 'ccusage']) {
      setPluginStatus(prev => {
        const existing = prev[name];
        if (existing) {
          return {
            ...prev,
            [name]: { ...existing, loading: true }
          };
        }
        return prev;
      });
      try {
        const ver = await api.plugins.detect(name);
        setPluginStatus(prev => {
          const existing = prev[name];
          if (existing) {
            return {
              ...prev,
              [name]: { installed: ver !== null, version: ver, loading: false }
            };
          }
          return prev;
        });
      } catch {
        setPluginStatus(prev => {
          const existing = prev[name];
          if (existing) {
            return {
              ...prev,
              [name]: { installed: false, version: null, loading: false }
            };
          }
          return prev;
        });
      }
    }
  }

  async function handleInstallPlugin(name: string) {
    const api = window.nativesAPI;
    if (!api?.plugins?.install) return;
    setActiveInstallPlugin(name);
    setInstallLogs([`Starting installation for plugin "${name}"...`]);
    try {
      await api.plugins.install(name);
    } catch (err: any) {
      globalToast(classifyError(err).userMessage, 'error');
      setActiveInstallPlugin(null);
    }
  }

  async function handleUninstallPlugin(name: string) {
    const api = window.nativesAPI;
    if (!api?.plugins?.uninstall) return;
    setActiveInstallPlugin(name);
    setInstallLogs([`Uninstalling plugin "${name}"...`]);
    try {
      await api.plugins.uninstall(name);
    } catch (err: any) {
      globalToast(classifyError(err).userMessage, 'error');
      setActiveInstallPlugin(null);
    }
  }

  async function handleThemeChange(theme: string) {
    try {
      setTheme(theme);
      globalToast(locale === 'zh' ? '主题修改成功' : 'Theme updated successfully', 'success');
    } catch (err: any) {
      globalToast(classifyError(err).userMessage, 'error');
    }
  }

  async function loadProviders() {
    setProvidersLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.provider?.unifiedList) setProviders(Array.isArray(await api.provider.unifiedList()) ? await api.provider.unifiedList() as ProviderSummary[] : []);
      else if (api?.provider?.list) setProviders(Array.isArray(await api.provider.list()) ? await api.provider.list() as ProviderSummary[] : []);
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
    finally { setProvidersLoading(false); }
  }

  async function handleSaveProvider(data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) {
    const api = window.nativesAPI;
    if (api?.provider?.create) {
      await api.provider.create({ providerType: data.presetName, displayName: data.name, websiteUrl: data.websiteUrl, baseUrl: data.baseUrl, initialKey: data.keys[0] ? { label: data.keys[0].label, apiKey: data.keys[0].apiKey } : null });
    } else if (api?.provider?.add) { await api.provider.add(data); }
    else throw new Error('Provider API not available');
    globalToast(t(locale, 'settings.providerAdded'), 'success');
    await loadProviders();
  }

  async function handleDeleteProvider(id: string) { if (!window.nativesAPI?.provider?.delete) return; await window.nativesAPI.provider.delete(id); globalToast(t(locale, 'settings.providerDeleted'), 'success'); await loadProviders(); }
  async function handleSaveDefaults(pid: string, m: string | null) { const a = window.nativesAPI; if (a?.provider?.updateDefaults) { await a.provider.updateDefaults({ providerId: pid, defaultModel: m }); await loadProviders(); } else throw new Error('updateDefaults'); }
  async function handleAddKey(pid: string, l: string, k: string) { const a = window.nativesAPI; if (a?.provider?.addKeyUnified) await a.provider.addKeyUnified({ providerId: pid, label: l, apiKey: k }); else if (a?.provider?.addKey) await a.provider.addKey({ providerId: pid, label: l, apiKey: k }); else throw new Error('addKey'); await loadProviders(); }
  async function handleTestKey(pid: string, kid: string): Promise<TestKeyResult> { const a = window.nativesAPI; if (a?.provider?.testKey) { const r = await a.provider.testKey({ providerId: pid, keyId: kid }); await loadProviders(); return r as unknown as TestKeyResult; } if (a?.provider?.test) { const r = await a.provider.test({ providerId: pid, keyId: kid }); await loadProviders(); return { success: r.success, status: r.success ? 'valid' as const : 'invalid' as const, testedAt: new Date().toISOString(), errorCode: r.success ? null : 'unknown', userMessage: r.error || null }; } throw new Error('testKey'); }
  async function handleSetPrimaryKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.setPrimaryKey) { await a.provider.setPrimaryKey({ providerId: pid, keyId: kid }); await loadProviders(); } else throw new Error('setPrimaryKey'); }
  async function handleDeleteKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.deleteKeyUnified) await a.provider.deleteKeyUnified({ providerId: pid, keyId: kid }); else if (a?.provider?.deleteKey) await a.provider.deleteKey(kid); else throw new Error('deleteKey'); await loadProviders(); }

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div style={{ padding: `${SPACING.lg}px 20px`, display: 'flex', flexDirection: 'column', gap: 28 }}>
        

        {/* ── Theme and Style Settings Tab ── */}
        <div style={{ display: activeTab === 'theme' ? 'block' : 'none' }}>
          <h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, marginBottom: SPACING.md }}>{t(locale, 'settings.theme')}</h2>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, maxWidth: 640 }}>
            {/* Light Theme Card */}
            <div
              onClick={() => handleThemeChange('light')}
              style={{
                padding: '24px 20px',
                borderRadius: '12px',
                border: `2px solid ${themeId === 'light' ? 'var(--text)' : 'var(--border)'}`,
                background: '#F4F4F2',
                color: '#111111',
                cursor: 'pointer',
                position: 'relative',
                transition: 'all 0.15s ease',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                <span style={{ fontWeight: 700, fontSize: '15px' }}>{locale === 'zh' ? '浅色模式' : 'Light Mode'}</span>
                {themeId === 'light' && <Check size={16} />}
              </div>
              <p style={{ fontSize: '11px', opacity: 0.7 }}>{locale === 'zh' ? '纯净高对比度浅色底纹' : 'Clean high contrast light workspace'}</p>
            </div>

            {/* Dark Theme Card */}
            <div
              onClick={() => handleThemeChange('dark')}
              style={{
                padding: '24px 20px',
                borderRadius: '12px',
                border: `2px solid ${themeId === 'dark' ? 'var(--text)' : 'var(--border)'}`,
                background: '#151515',
                color: '#F5F5F5',
                cursor: 'pointer',
                position: 'relative',
                transition: 'all 0.15s ease',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                <span style={{ fontWeight: 700, fontSize: '15px' }}>{locale === 'zh' ? '深色模式' : 'Dark Mode'}</span>
                {themeId === 'dark' && <Check size={16} />}
              </div>
              <p style={{ fontSize: '11px', opacity: 0.7 }}>{locale === 'zh' ? '克制护眼暗黑开发者风格' : 'Sleek dark theme developer style'}</p>
            </div>
          </div>
        </div>

        {/* ── Environment Variables Tab ── */}
        <div style={{ display: activeTab === 'env' ? 'block' : 'none' }}>
          <h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, marginBottom: SPACING.md }}>{t(locale, 'settings.environment')}</h2>
          <div style={{ padding: 24, borderRadius: 12, border: '1px solid var(--border)', background: 'var(--bg-2)', maxWidth: 640 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
              <Shield size={18} style={{ color: 'var(--accent)' }} />
              <span style={{ fontWeight: 600, fontSize: '13px' }}>{locale === 'zh' ? '数据保险箱加密保护' : 'AES-256-GCM Secure Vault'}</span>
            </div>
            <p style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.5 }}>
              {locale === 'zh' 
                ? '所有 API 服务商凭证已在本地通过硬件绑定的 AES-256-GCM 算法进行高强度加密。您可以点击右上方的「服务商」页签来直接维护您的秘钥和端点。'
                : 'All API keys are encrypted locally using AES-256-GCM hardware-bound keys. Switch to the "Providers" tab above to manage your endpoints and credentials.'}
            </p>
          </div>
        </div>

        {/* ── Plugin Settings Tab ── */}
        <div style={{ display: activeTab === 'plugins' ? 'block' : 'none' }}>
          <h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, marginBottom: SPACING.md }}>{t(locale, 'settings.tabPlugins')}</h2>
          
          {/* Plugins List */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 720 }}>
            {Object.entries(pluginStatus).map(([name, status]) => {
              const description = name === 'rtk'
                ? (locale === 'zh' ? 'TypeScript 类型诊断加速代理，按文件分组提取编译错误' : 'TypeScript compiler diagnosis proxy, aggregates errors by file')
                : name === 'codegraph'
                ? (locale === 'zh' ? 'CodeGraph 代码图分析器，极速分析项目符号依赖与调用链路' : 'CodeGraph analyzer, explores symbol dependencies and call chains')
                : (locale === 'zh' ? 'Vibe 跨工具用量与成本分摊计算引擎，提供后台日志每日核算' : 'Vibe cross-tool usage and cost allocation engine, reconciles daily logs');

              const isRunning = activeInstallPlugin === name;

              return (
                <div
                  key={name}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    padding: '16px 20px',
                    borderRadius: '12px',
                    border: '1px solid var(--border)',
                    background: 'var(--bg-2)',
                  }}
                >
                  <div style={{ flex: 1, paddingRight: 20 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                      <span style={{ fontWeight: 700, fontSize: '14px', fontFamily: 'var(--font-mono)' }}>{name}</span>
                      {status.loading ? (
                        <span style={{ fontSize: '10px', color: 'var(--text-disabled)' }}>Detecting...</span>
                      ) : status.installed ? (
                        <span style={{ fontSize: '10px', color: 'var(--success)', fontWeight: 600, background: 'var(--success-soft)', padding: '2px 6px', borderRadius: 4 }}>
                          {locale === 'zh' ? '已安装' : 'Installed'} {status.version}
                        </span>
                      ) : (
                        <span style={{ fontSize: '10px', color: 'var(--text-disabled)', background: 'var(--bg-3)', padding: '2px 6px', borderRadius: 4 }}>
                          {locale === 'zh' ? '未安装' : 'Not Installed'}
                        </span>
                      )}
                    </div>
                    <p style={{ fontSize: '11px', color: 'var(--text-secondary)' }}>{description}</p>
                  </div>

                  {/* Actions */}
                  <div style={{ flexShrink: 0 }}>
                    {isRunning ? (
                      <button
                        disabled
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 6,
                          padding: '6px 16px',
                          borderRadius: 20,
                          border: '1px solid var(--border)',
                          background: 'var(--bg-3)',
                          color: 'var(--text-disabled)',
                          fontSize: '11px',
                        }}
                      >
                        <Loader2 size={12} style={{ animation: 'spin 1s linear infinite' }} />
                        {locale === 'zh' ? '执行中...' : 'Processing...'}
                      </button>
                    ) : status.installed ? (
                      <button
                        onClick={() => handleUninstallPlugin(name)}
                        style={{
                          padding: '6px 16px',
                          borderRadius: 20,
                          border: '1px solid var(--danger)',
                          background: 'transparent',
                          color: 'var(--danger)',
                          fontSize: '11px',
                          fontWeight: 500,
                          cursor: 'pointer',
                        }}
                      >
                        {locale === 'zh' ? '卸载' : 'Uninstall'}
                      </button>
                    ) : (
                      <button
                        onClick={() => handleInstallPlugin(name)}
                        style={{
                          padding: '6px 16px',
                          borderRadius: 20,
                          border: '1px solid var(--text)',
                          background: 'var(--text)',
                          color: 'var(--bg)',
                          fontSize: '11px',
                          fontWeight: 600,
                          cursor: 'pointer',
                        }}
                      >
                        {locale === 'zh' ? '安装' : 'Install'}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Installation Terminal Logs console */}
          {activeInstallPlugin && (
            <div style={{ marginTop: 20, maxWidth: 720 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: '11px', color: 'var(--text-dim)', marginBottom: 6 }}>
                <TermIcon size={12} />
                <span>{locale === 'zh' ? '安装控制台输出' : 'Installation Output Logs'}</span>
              </div>
              <div
                style={{
                  padding: 12,
                  borderRadius: 8,
                  background: '#0d0d0d',
                  border: '1px solid #222',
                  maxHeight: 200,
                  overflowY: 'auto',
                  fontFamily: 'var(--font-mono)',
                  fontSize: '10px',
                  color: '#00ff66',
                  whiteSpace: 'pre-wrap',
                  lineHeight: 1.5,
                }}
              >
                {installLogs.map((log, index) => (
                  <div key={index}>{log}</div>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* ── Executor Tab ── */}
        <div style={{ display: activeTab === 'executor' ? 'block' : 'none' }}>
          <RuntimePanel locale={locale} />
        </div>

        {/* ── Providers Tab ── */}
        <div style={{ display: activeTab === 'providers' ? 'block' : 'none' }}>
          <ProviderDetail
            locale={locale} providers={providers} loading={providersLoading}
            showAddProvider={() => setShowAddProvider(true)}
            onSaveDefaults={handleSaveDefaults} onAddKey={handleAddKey} onTestKey={handleTestKey}
            onSetPrimaryKey={handleSetPrimaryKey} onDeleteKey={handleDeleteKey}
            onDeleteProvider={(id) => setDeleteProviderTarget(id)}
          />
        </div>

      </div>
      {showAddProvider && <AddProviderDialog locale={locale} onClose={() => setShowAddProvider(false)} onSave={handleSaveProvider} />}
      <ConfirmDialog open={deleteProviderTarget !== null} title={t(locale, 'settings.deleteProvider')} message={t(locale, 'settings.confirmDeleteProvider')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (deleteProviderTarget) handleDeleteProvider(deleteProviderTarget); setDeleteProviderTarget(null); }}
        onCancel={() => setDeleteProviderTarget(null)} />
    </div>
  );
}
