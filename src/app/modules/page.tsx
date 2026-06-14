'use client';

import { useState, useEffect } from 'react';
import { Power, Trash2, RefreshCw } from 'lucide-react';

interface ModuleInfo {
  id: string;
  name: string;
  version: string;
  enabled: number;
  state: string;
  description?: string;
  author?: string;
}

export default function ModulesPage() {
  const [modules, setModules] = useState<ModuleInfo[]>([]);
  const [loading, setLoading] = useState(true);

  async function loadModules() {
    setLoading(true);
    try {
      const api = (window as any).nativesAPI;
      if (api?.module?.list) {
        const result = await api.module.list();
        if (Array.isArray(result)) {
          setModules(result);
        }
      }
    } catch (err) {
      console.error('[Modules] Failed to load modules:', err);
    }
    setLoading(false);
  }

  useEffect(() => {
    loadModules();
  }, []);

  async function handleToggle(mod: ModuleInfo) {
    try {
      const api = (window as any).nativesAPI;
      if (mod.enabled) {
        await api?.module?.disable?.(mod.id);
      } else {
        await api?.module?.enable?.(mod.id);
      }
      await loadModules();
    } catch (err) {
      console.error('[Modules] Toggle failed:', err);
    }
  }

  async function handleUninstall(mod: ModuleInfo) {
    if (!confirm(`Are you sure you want to uninstall "${mod.name}"?`)) return;
    try {
      const api = (window as any).nativesAPI;
      await api?.module?.uninstall?.(mod.id);
      await loadModules();
    } catch (err) {
      console.error('[Modules] Uninstall failed:', err);
    }
  }

  async function handleScan() {
    try {
      const api = (window as any).nativesAPI;
      await api?.module?.scan?.();
      await loadModules();
    } catch (err) {
      console.error('[Modules] Scan failed:', err);
    }
  }

  return (
    <div style={{ height: '100%', overflow: 'auto' }} role="region" aria-label="Module manager">
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>Module Manager</h1>
        <button
          onClick={handleScan}
          style={{ background: 'var(--bg-3,#1c1e17)', border: '1px solid var(--border,#262920)', color: 'var(--text,#f2f2ea)', padding: '6px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 6 }}
          aria-label="Scan for modules"
        >
          <RefreshCw size={14} />
          Scan
        </button>
      </div>

      {loading ? (
        <div style={{ padding: '60px 20px', textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13 }}>
          Loading...
        </div>
      ) : modules.length === 0 ? (
        <div style={{ padding: '60px 20px', textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13 }}>
          No modules installed. Visit the Workshop to install modules.
        </div>
      ) : (
        <div aria-live="polite">
          {modules.map((mod) => (
            <div key={mod.id} style={{
              display: 'flex', alignItems: 'center', gap: 12,
              padding: '12px 20px', borderBottom: '1px solid var(--border,#262920)',
            }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text,#f2f2ea)' }}>{mod.name}</div>
                <div style={{ fontSize: 11, color: 'var(--text-faint,#62655a)' }}>{mod.id} v{mod.version}</div>
                {mod.description && <div style={{ fontSize: 12, color: 'var(--text-dim,#9b9d8c)', marginTop: 4 }}>{mod.description}</div>}
              </div>
              <span style={{
                fontSize: 11, padding: '2px 8px', borderRadius: 4,
                background: mod.enabled ? 'var(--accent-soft,#cdf24b1f)' : 'var(--bg-3,#1c1e17)',
                color: mod.enabled ? 'var(--accent,#cdf24b)' : 'var(--text-faint,#62655a)',
              }}>
                {mod.enabled ? 'Enabled' : 'Disabled'}
              </span>
              <button
                onClick={() => handleToggle(mod)}
                style={{ background: 'none', border: '1px solid var(--border,#262920)', color: 'var(--text-dim,#9b9d8c)', padding: '4px 8px', borderRadius: 4, cursor: 'pointer' }}
                aria-label={mod.enabled ? `Disable ${mod.name}` : `Enable ${mod.name}`}
              >
                <Power size={14} />
              </button>
              <button
                onClick={() => handleUninstall(mod)}
                style={{ background: 'none', border: '1px solid var(--border,#262920)', color: '#e06a5b', padding: '4px 8px', borderRadius: 4, cursor: 'pointer' }}
                aria-label={`Uninstall ${mod.name}`}
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
