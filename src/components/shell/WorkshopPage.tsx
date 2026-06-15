'use client';

import { useState, useEffect, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { useFocusTrap } from '@/lib/useFocusTrap';

interface ModuleInfo {
  id: string;
  name: string;
  version: string;
  enabled: number;
  state: string;
  description?: string;
  author?: string;
}

interface WorkshopPageProps {
  onInstall: (source: string) => void;
}

export default function WorkshopPage({ onInstall }: WorkshopPageProps) {
  // onInstall (legacy direct-install callback) is intentionally unused: all
  // installs now flow through the permission dialog to avoid bypassing
  // authorization. Kept in the props type for ShellLayout compatibility.
  void onInstall;
  const [dragOver, setDragOver] = useState(false);
  const [modules, setModules] = useState<ModuleInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [templateName, setTemplateName] = useState('');
  const [templateId, setTemplateId] = useState('');
  const [creating, setCreating] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const [currentUser, setCurrentUser] = useState('You');

  // Permission dialog state (US12)
  const [permDialog, setPermDialog] = useState<{
    source: string;
    moduleName: string;
    permissions: string[];
  } | null>(null);
  const [installing, setInstalling] = useState(false);
  // P1-3: Track which permissions the user has selected (checkboxes)
  const [selectedPerms, setSelectedPerms] = useState<Set<string>>(new Set());

  // Focus traps (STYLE-2)
  const createDialogTrap = useFocusTrap();
  const permDialogTrap = useFocusTrap();

  // areaRipple animation state (STYLE-1)
  const [showRipple, setShowRipple] = useState(false);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2200);
  }, []);

  // Load locale & current user
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* browser dev mode */ }
    }
    async function loadUser() {
      try {
        const name = await window.nativesAPI?.db?.get?.('settings:username');
        if (name) setCurrentUser(name as string);
      } catch { /* ignore */ }
    }
    loadLocale();
    loadUser();
  }, []);

  const loadModules = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.module?.list) {
        const result = await api.module.list();
        if (Array.isArray(result)) {
          setModules(result as ModuleInfo[]);
        }
      }
    } catch (err) {
      console.error('[Workshop] Failed to load modules:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadModules();
  }, [loadModules]);

  // Drag & drop
  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  };

  const handleDragLeave = () => {
    setDragOver(false);
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer.files);
    for (const file of files) {
      if (file.name.endsWith('.zip') || file.type === '') {
        const source = file.path || file.name;
        // Read manifest to show the permission dialog (US12).
        // If the manifest cannot be read (corrupt zip / missing manifest.json)
        // we must NOT silently install — that would bypass the permission
        // confirmation. Instead surface the error and abort. See BUG-2.
        try {
          const api = window.nativesAPI;
          const result = await api?.module?.readManifest?.(source);
          if (result?.manifest) {
            const perms = result.manifest.permissions || [];
            setPermDialog({
              source,
              moduleName: result.manifest.name,
              permissions: perms,
            });
            // P1-3: Default all permissions selected
            setSelectedPerms(new Set(perms));
          } else {
            // Manifest unreadable: refuse to install rather than bypassing
            // the permission step. Surface the error reason if available.
            const reason = (result as { error?: string } | undefined)?.error;
            showToast(
              reason
                ? t(locale, 'errors.installFailed').replace('{reason}', reason)
                : t(locale, 'workshop.invalidPackage').replace('{name}', file.name)
            );
          }
        } catch (err) {
          const reason = (err as Error)?.message || String(err);
          showToast(t(locale, 'errors.installFailed').replace('{reason}', reason));
        }
      }
    }
  };

  const handlePermInstall = async (allowAll: boolean) => {
    if (!permDialog) return;
    setInstalling(true);
    try {
      const api = window.nativesAPI;
      const installResult = await api?.module?.install?.(permDialog.source);
      if (installResult?.success) {
        // Grant only selected permissions (or all if allowAll)
        const toGrant = allowAll ? permDialog.permissions : Array.from(selectedPerms);
        for (const perm of toGrant) {
          await api?.module?.grantPermission?.(installResult.moduleId!, perm);
        }
        showToast(t(locale, 'workshop.installSuccess'));
        setShowRipple(true);
        setTimeout(() => setShowRipple(false), 1200);
        await loadModules();
      } else {
        showToast(t(locale, 'workshop.installFailed'));
      }
    } catch (err) {
      console.error('[Workshop] Install error:', err);
      showToast(t(locale, 'workshop.installFailed'));
    } finally {
      setInstalling(false);
      setPermDialog(null);
    }
  };

  // Module actions
  const handleToggle = async (mod: ModuleInfo) => {
    try {
      const api = window.nativesAPI;
      if (mod.enabled) {
        await api?.module?.disable?.(mod.id);
      } else {
        await api?.module?.enable?.(mod.id);
      }
      await loadModules();
    } catch (err) {
      console.error('[Workshop] Toggle failed:', err);
    }
  };

  const handleUninstall = async (mod: ModuleInfo) => {
    if (!confirm(t(locale, 'workshop.confirmUninstall'))) return;
    try {
      await window.nativesAPI?.module?.uninstall?.(mod.id);
      await loadModules();
      showToast(t(locale, 'modules.uninstall'));
    } catch (err) {
      console.error('[Workshop] Uninstall failed:', err);
    }
  };

  const handleScan = async () => {
    try {
      await window.nativesAPI?.module?.scan?.();
      await loadModules();
      showToast(t(locale, 'workshop.modulesFound').replace('{count}', String(modules.length)));
    } catch (err) {
      console.error('[Workshop] Scan failed:', err);
    }
  };

  // Template creation
  const handleCreateTemplate = async () => {
    const name = templateName.trim();
    const id = templateId.trim();
    if (!name || !id) return;

    setCreating(true);
    try {
      // Create module directory via fs API
      const api = window.nativesAPI;
      const home = await api?.db?.get?.('settings:home_dir') || '~/.natives';
      const modulePath = `${home}/modules/${id}`;

      // Create the module structure
      await api?.fs?.createEntry?.(modulePath, 'directory');

      // Write manifest.json
      const manifest = {
        id,
        name,
        version: '0.1.0',
        entry: 'index.html',
        type: 'page',
        permissions: ['db', 'settings', 'lifecycle'],
        description: `${name} - A Natives module`,
        author: currentUser || 'Anonymous',
        lifecycle: {
          heartbeatInterval: 5000,
          loadTimeout: 10000,
        },
      };
      await api?.fs?.writeFileAtomic?.(
        `${modulePath}/manifest.json`,
        JSON.stringify(manifest, null, 2)
      );

      // Write index.html
      const html = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${name}</title>
  <style>
    body { font-family: -apple-system, sans-serif; padding: 20px; color: #f2f2ea; background: #0b0c0a; }
    h1 { color: #cdf24b; }
    .card { background: #131410; border: 1px solid #262920; border-radius: 8px; padding: 16px; margin: 12px 0; }
    button { background: #cdf24b; color: #0b0c0a; border: none; padding: 8px 16px; border-radius: 6px; cursor: pointer; font-weight: 600; }
    button:hover { filter: brightness(1.1); }
  </style>
</head>
<body>
  <h1>${name}</h1>
  <p>Your Natives module is ready!</p>
  <div class="card">
    <h3>Bridge API Demo</h3>
    <p>Module ID: <code id="moduleId">loading...</code></p>
    <button onclick="testStorage()">Test Data Storage</button>
    <p id="result"></p>
  </div>
  <script>
    // Wait for Bridge SDK to load
    window.addEventListener('message', (e) => {
      if (e.data?.type === 'lifecycle:ready') {
        document.getElementById('moduleId').textContent = window.natives?.meta?.moduleId || 'unknown';
      }
    });
    async function testStorage() {
      const key = 'test-key';
      const value = 'Hello from ${name}! ' + new Date().toLocaleTimeString();
      await window.natives?.db?.set(key, value);
      const stored = await window.natives?.db?.get(key);
      document.getElementById('result').textContent = 'Stored: ' + stored;
    }
  </script>
</body>
</html>`;
      await api?.fs?.writeFileAtomic?.(`${modulePath}/index.html`, html);

      // Write README.md
      const readme = `# ${name}

A Natives module.

## Development

Edit \`index.html\` to customize your module. The Bridge API is available via \`window.natives.*\`.

## Bridge API

- \`window.natives.db.get(key)\` — Read data
- \`window.natives.db.set(key, value)\` — Write data
- \`window.natives.settings.getTheme()\` — Get current theme
- \`window.natives.lifecycle.ready()\` — Signal ready state
`;
      await api?.fs?.writeFileAtomic?.(`${modulePath}/README.md`, readme);

      showToast(t(locale, 'workshop.templateCreated'));
      setShowCreateDialog(false);
      setTemplateName('');
      setTemplateId('');

      // Re-scan to pick up the new module
      await api?.module?.scan?.();
      await loadModules();
    } catch (err) {
      console.error('[Workshop] Create template failed:', err);
    } finally {
      setCreating(false);
    }
  };

  // Auto-generate ID from name
  const handleNameChange = (name: string) => {
    setTemplateName(name);
    if (!templateId || templateId === generateId(templateName)) {
      setTemplateId(generateId(name));
    }
  };

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      {/* Header */}
      <div style={{
        padding: '16px 20px',
        borderBottom: '1px solid var(--border,#262920)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        flexShrink: 0,
      }}>
        <div>
          <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>
            {t(locale, 'workshop.title')}
          </h1>
          <p style={{ fontSize: 12, color: 'var(--text-dim,#9b9d8c)', margin: '2px 0 0' }}>
            {t(locale, 'workshop.subtitle')}
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn" onClick={handleScan} style={{ fontSize: 12 }}>
            ↻ {t(locale, 'workshop.scanModules')}
          </button>
          <button className="btn btn-primary" onClick={() => setShowCreateDialog(true)} style={{ fontSize: 12 }}>
            + {t(locale, 'workshop.createModule')}
          </button>
        </div>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'auto', padding: '16px 20px', position: 'relative' }}>
        {/* Drop zone */}
        <div
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          style={{
            padding: dragOver ? 36 : 28,
            border: `2px dashed ${dragOver ? 'var(--accent,#cdf24b)' : 'var(--border,#262920)'}`,
            borderRadius: 8,
            textAlign: 'center',
            color: 'var(--text-faint,#62655a)',
            fontSize: 13,
            transition: 'all 0.16s cubic-bezier(0.2,0.7,0.3,1)',
            background: dragOver ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
            marginBottom: 20,
          }}
        >
          {dragOver ? (
            <span style={{ color: 'var(--accent,#cdf24b)', fontWeight: 600 }}>
              {t(locale, 'workshop.releaseToInstall')}
            </span>
          ) : (
            <>
              <div style={{ fontSize: 24, marginBottom: 6 }}>📦</div>
              <div>{t(locale, 'workshop.dragToInstall')}</div>
            </>
          )}
        </div>

        {/* areaRipple overlay on install success (STYLE-1) */}
        {showRipple && (
          <div className="anim-areaRipple" style={{
            position: 'absolute', inset: 0, borderRadius: 8,
            pointerEvents: 'none', zIndex: 5,
          }} />
        )}

        {/* Module grid */}
        {loading ? (
          <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-faint)', fontSize: 13 }}>
            {t(locale, 'common.loading')}
          </div>
        ) : modules.length === 0 ? (
          <div style={{
            padding: '40px 0',
            textAlign: 'center',
            color: 'var(--text-faint,#62655a)',
            fontSize: 13,
          }}>
            <div style={{ fontSize: 32, marginBottom: 8 }}>🧩</div>
            <div>{t(locale, 'workshop.noModules')}</div>
            <div style={{ fontSize: 11, marginTop: 4 }}>{t(locale, 'workshop.noModulesDesc')}</div>
          </div>
        ) : (
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))',
            gap: 12,
          }}>
            {modules.map((mod) => (
              <ModuleCard
                key={mod.id}
                module={mod}
                locale={locale}
                onToggle={() => handleToggle(mod)}
                onUninstall={() => handleUninstall(mod)}
              />
            ))}
          </div>
        )}
      </div>

      {/* Create template dialog */}
      {showCreateDialog && (
        <div
          ref={createDialogTrap.dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label={t(locale, 'workshop.createModule')}
          style={{
            position: 'fixed', inset: 0,
            background: 'rgba(0,0,0,0.5)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            zIndex: 60,
          }}
          onClick={() => setShowCreateDialog(false)}
          onKeyDown={(e) => {
            createDialogTrap.handleKeyDown(e);
            if (e.key === 'Escape') setShowCreateDialog(false);
          }}
        >
          <div
            style={{
              background: 'var(--bg-2,#131410)',
              border: '1px solid var(--border,#262920)',
              borderRadius: 12,
              padding: 24,
              width: 400,
              maxWidth: '90vw',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <h2 style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', margin: '0 0 16px' }}>
              {t(locale, 'workshop.createModule')}
            </h2>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div>
                <label style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 1, marginBottom: 4, display: 'block' }}>
                  {t(locale, 'workshop.templateName')}
                </label>
                <input
                  type="text"
                  value={templateName}
                  onChange={(e) => handleNameChange(e.target.value)}
                  placeholder={t(locale, 'workshop.templateNamePlaceholder')}
                  style={dialogInputStyle}
                  autoFocus
                />
              </div>

              <div>
                <label style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 1, marginBottom: 4, display: 'block' }}>
                  {t(locale, 'workshop.templateId')}
                </label>
                <input
                  type="text"
                  value={templateId}
                  onChange={(e) => setTemplateId(e.target.value)}
                  placeholder={t(locale, 'workshop.templateIdPlaceholder')}
                  style={dialogInputStyle}
                />
              </div>
            </div>

            <div style={{ display: 'flex', gap: 8, marginTop: 20, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setShowCreateDialog(false)}>
                {t(locale, 'common.cancel')}
              </button>
              <button
                className="btn btn-primary"
                onClick={handleCreateTemplate}
                disabled={creating || !templateName.trim() || !templateId.trim()}
              >
                {creating ? t(locale, 'workshop.creating') : t(locale, 'workshop.createTemplate')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Permission dialog (US12) */}
      {permDialog && (
        <div
          ref={permDialogTrap.dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label={t(locale, 'workshop.permissionTitle')}
          style={{
            position: 'fixed', inset: 0,
            background: 'rgba(0,0,0,0.5)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            zIndex: 60,
          }}
          onClick={() => setPermDialog(null)}
          onKeyDown={(e) => {
            permDialogTrap.handleKeyDown(e);
            if (e.key === 'Escape') setPermDialog(null);
          }}
        >
          <div style={{
            background: 'var(--bg-2,#131410)',
            border: '1px solid var(--border,#262920)',
            borderRadius: 12,
            padding: 24,
            width: 420,
            maxWidth: '90vw',
          }}>
            <h2 style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', margin: '0 0 16px' }}>
              {t(locale, 'workshop.permissionTitle')}
            </h2>
            <p style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 16 }}>
              {t(locale, 'workshop.permissionDesc').replace('{name}', permDialog.moduleName)}
            </p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 20 }}>
              {permDialog.permissions.map((perm) => (
                <label key={perm} style={{
                  display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer',
                  padding: '8px 10px',
                  background: selectedPerms.has(perm) ? 'var(--accent-soft,#cdf24b1f)' : 'var(--bg-3,#1c1e17)',
                  borderRadius: 6,
                  fontSize: 12,
                  transition: 'background 0.12s',
                }}>
                  <input
                    type="checkbox"
                    checked={selectedPerms.has(perm)}
                    onChange={() => {
                      setSelectedPerms((prev) => {
                        const next = new Set(prev);
                        if (next.has(perm)) next.delete(perm); else next.add(perm);
                        return next;
                      });
                    }}
                  />
                  <span style={{ color: 'var(--text)', fontFamily: 'var(--font-mono)', fontSize: 11 }}>
                    {perm}
                  </span>
                  <span style={{ color: 'var(--text-faint)', marginLeft: 'auto', fontSize: 11 }}>
                    {PERMISSION_DESC[perm as keyof typeof PERMISSION_DESC] || perm}
                  </span>
                </label>
              ))}
            </div>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => { setPermDialog(null); setInstalling(false); }} disabled={installing}>
                {t(locale, 'common.cancel')}
              </button>
              <button className="btn" onClick={() => handlePermInstall(false)} disabled={installing || selectedPerms.size === 0}>
                {installing ? t(locale, 'common.loading') : `Allow Selected (${selectedPerms.size})`}
              </button>
              <button className="btn btn-primary" onClick={() => handlePermInstall(true)} disabled={installing}>
                {installing ? t(locale, 'common.loading') : t(locale, 'workshop.permissionAllowAll')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Toast */}
      {toast && (
        <div style={{
          position: 'fixed', bottom: 24, left: '50%', transform: 'translateX(-50%)',
          background: 'var(--bg-3,#1c1e17)', border: '1px solid var(--border,#262920)',
          padding: '10px 18px', borderRadius: 10, fontSize: 13, color: 'var(--text)',
          zIndex: 200, animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        }}>
          {toast}
        </div>
      )}
    </div>
  );
}


// ── Module Card ──

function ModuleCard({
  module: mod,
  locale,
  onToggle,
  onUninstall,
}: {
  module: ModuleInfo;
  locale: Locale;
  onToggle: () => void;
  onUninstall: () => void;
}) {
  const [hovered, setHovered] = useState(false);

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: 'var(--bg-2,#131410)',
        border: '1px solid var(--border,#262920)',
        borderRadius: 8,
        padding: '14px 12px',
        transition: 'all 0.12s',
        borderColor: hovered ? 'var(--accent,#cdf24b)' : undefined,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 8 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            fontSize: 13, fontWeight: 600, color: 'var(--text)',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
            {mod.name}
          </div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
            {mod.id} · v{mod.version}
          </div>
        </div>
        <span style={{
          fontSize: 9, padding: '1px 5px', borderRadius: 3,
          background: mod.enabled ? 'var(--accent-soft,#cdf24b1f)' : 'var(--bg-3,#1c1e17)',
          color: mod.enabled ? 'var(--accent,#cdf24b)' : 'var(--text-faint)',
          fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5, flexShrink: 0,
        }}>
          {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
        </span>
      </div>

      {mod.description && (
        <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 10, lineHeight: 1.4 }}>
          {mod.description}
        </div>
      )}

      <div style={{
        display: 'flex', gap: 6,
        opacity: hovered ? 1 : 0.3,
        transition: 'opacity 0.12s',
      }}>
        <button
          className="btn"
          onClick={onToggle}
          style={{ flex: 1, fontSize: 11, padding: '4px 8px' }}
          title={t(locale, 'workshop.toggleModule')}
        >
          {mod.enabled ? '⏸' : '▶'} {mod.enabled ? t(locale, 'modules.disable') : t(locale, 'modules.enable')}
        </button>
        <button
          className="btn"
          onClick={onUninstall}
          style={{ fontSize: 11, padding: '4px 8px', color: '#e06a5b' }}
          title={t(locale, 'workshop.uninstallModule')}
        >
          🗑
        </button>
      </div>
    </div>
  );
}

// ── Permission descriptions (US12) ──

const PERMISSION_DESC: Record<string, string> = {
  'db:read': '读取数据存储',
  'db:write': '写入数据存储',
  'env:read': '读取环境变量',
  'notification': '发送通知',
  'ipc:send': '模块间通信',
  'lifecycle': '生命周期管理',
  'settings': '访问设置',
};

// ── Helpers ──

function generateId(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    || 'my-module';
}

const dialogInputStyle: React.CSSProperties = {
  width: '100%',
  padding: '8px 10px',
  background: 'var(--bg,#0b0c0a)',
  border: '1px solid var(--border,#262920)',
  borderRadius: 6,
  color: 'var(--text)',
  fontSize: 13,
  outline: 'none',
};
