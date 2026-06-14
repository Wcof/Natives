'use client';

import { useEffect, useState, useCallback } from 'react';
import { Settings, Layers, ChevronRight, Square, ShoppingBag, Bell, Folder, Star, Home, Monitor, FileText, Download } from 'lucide-react';
import { t, type Locale } from '@/i18n';

interface ModuleItem {
  id: string;
  name: string;
  icon?: string;
}

const QUICK_ACCESS_DIRS = [
  { id: 'home', label: 'Home', path: '~', icon: <Home size={16} /> },
  { id: 'desktop', label: 'Desktop', path: '~/Desktop', icon: <Monitor size={16} /> },
  { id: 'documents', label: 'Documents', path: '~/Documents', icon: <FileText size={16} /> },
  { id: 'downloads', label: 'Downloads', path: '~/Downloads', icon: <Download size={16} /> },
];

interface SidebarProps {
  isCollapsed: boolean;
  onToggle: () => void;
  width: number;
  onResize: (width: number) => void;
  activeModuleId?: string;
  onModuleSelect: (moduleId: string) => void;
  onNotificationClick?: () => void;
}

export default function Sidebar({
  isCollapsed,
  onToggle,
  width,
  onResize,
  activeModuleId,
  onModuleSelect,
  onNotificationClick,
}: SidebarProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [modules, setModules] = useState<ModuleItem[]>([]);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const [favorites, setFavorites] = useState<string[]>([]);

  // Load favorites from DB
  const loadFavorites = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (api?.db?.get) {
        const stored = await api.db.get('settings:favorites');
        if (stored) {
          setFavorites(JSON.parse(stored));
        }
      }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    loadFavorites();
  }, [loadFavorites]);

  // Load modules on mount
  useEffect(() => {
    async function loadModules() {
      try {
        const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;

        // Load locale
        if (api && typeof (api as Record<string, unknown>).getLocale === 'function') {
          const savedLocale = await (api as { getLocale: () => Promise<string> }).getLocale();
          if (savedLocale === 'en') setLocale('en');
        }

        // Load saved module order
        let savedOrder: string[] = [];
        if (api) {
          const dbApi = (api as Record<string, unknown>).db as { get?: (key: string) => Promise<string | undefined> } | undefined;
          if (dbApi?.get) {
            const raw = await dbApi.get('settings:module_order');
            if (raw) {
              try { savedOrder = JSON.parse(raw); } catch { /* ignore */ }
            }
          }
        }

        // Try to get modules from IPC
        if (api && typeof (api as Record<string, unknown>).module === 'object') {
          const modApi = (api as Record<string, unknown>).module as { scan?: () => Promise<ModuleItem[]> };
          if (modApi.scan) {
            const result = await modApi.scan();
            if (Array.isArray(result)) {
              if (savedOrder.length > 0) {
                const idIndex = new Map(savedOrder.map((id, i) => [id, i]));
                result.sort((a, b) => (idIndex.get(a.id) ?? Infinity) - (idIndex.get(b.id) ?? Infinity));
              }
              setModules(result);
              return;
            }
          }
        }
      } catch {
        // Fallback: no IPC available (browser dev mode)
      }
    }
    loadModules();
  }, []);

  // ── Drag resize ──
  const handleMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    const startX = e.clientX;
    const startW = width;

    const handleMove = (ev: MouseEvent) => {
      const newWidth = Math.max(190, Math.min(420, startW + (ev.clientX - startX)));
      onResize(newWidth);
    };
    const handleUp = () => {
      setIsDragging(false);
      document.removeEventListener('mousemove', handleMove);
      document.removeEventListener('mouseup', handleUp);
    };
    document.addEventListener('mousemove', handleMove);
    document.addEventListener('mouseup', handleUp);
  };

  // ── Module drag reorder ──
  const handleDragStart = (index: number) => {
    setDragIndex(index);
  };

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    if (dragIndex === null || dragIndex === index) return;
    const newModules = [...modules];
    const [moved] = newModules.splice(dragIndex, 1);
    newModules.splice(index, 0, moved!);
    setModules(newModules);
    setDragIndex(index);
  };

  const handleDragEnd = async () => {
    setDragIndex(null);
    // Persist new module order to database
    try {
      const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;
      if (api) {
        const db = (api as Record<string, unknown>).db as { set?: (key: string, value: unknown) => Promise<{ ok: boolean }> } | undefined;
        if (db?.set) {
          const orderedIds = modules.map((m) => m.id);
          await db.set('settings:module_order', orderedIds);
        }
      }
    } catch { /* browser dev mode */ }
  };

  return (
    <aside
      className={`sidebar ${isCollapsed ? 'collapsed' : ''}`}
      style={{ width: isCollapsed ? 0 : width }}
      role="navigation"
      aria-label="Module sidebar"
      data-sidebar
    >
      <div className="sidebar-header">
        <span className="sidebar-brand">NATIVES</span>
        <button className="btn-ghost" onClick={onToggle} aria-label="Toggle sidebar">
          <ChevronRight size={14} />
        </button>
      </div>

      <div className="sidebar-section-title" id="sidebar-modules-label">{t(locale, 'nav.modules')}</div>
      <div
        className="sidebar-modules"
        role="listbox"
        aria-labelledby="sidebar-modules-label"
        aria-live="polite"
        onDragEnd={handleDragEnd}
      >
        {modules.length === 0 ? (
          <div style={{ padding: '20px 12px', textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {t(locale, 'modules.noModules')}
          </div>
        ) : (
          modules.map((mod, index) => (
            <div
              key={mod.id}
              className={`sidebar-item ${activeModuleId === mod.id ? 'active' : ''}`}
              role="option"
              aria-selected={activeModuleId === mod.id}
              tabIndex={0}
              draggable
              onDragStart={() => handleDragStart(index)}
              onDragOver={(e) => handleDragOver(e, index)}
              onClick={() => onModuleSelect(mod.id)}
              onKeyDown={(e) => { if (e.key === 'Enter') onModuleSelect(mod.id); }}
              title={mod.name}
            >
              <span className="sidebar-module-icon">
                {mod.icon ? (
                  <img src={mod.icon} alt="" style={{ width: 18, height: 18 }} />
                ) : (
                  <Square size={18} />
                )}
              </span>
              <span className="sidebar-module-name">{mod.name}</span>
            </div>
          ))
        )}
      </div>

      {/* Quick Access */}
      <div className="sidebar-section-title">{t(locale, 'sidebar.quickAccess')}</div>
      <div style={{ padding: '0 4px' }}>
        {QUICK_ACCESS_DIRS.map((dir) => (
          <button
            key={dir.id}
            className="sidebar-item"
            onClick={() => onModuleSelect(`__files__:${dir.path}`)}
            style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}
            title={dir.path}
          >
            {dir.icon}
            <span style={{ fontSize: 12 }}>{dir.label}</span>
          </button>
        ))}
      </div>

      {/* Favorites */}
      {favorites.length > 0 && (
        <>
          <div className="sidebar-section-title">{t(locale, 'sidebar.favorites')}</div>
          <div style={{ padding: '0 4px' }}>
            {favorites.map((fav) => (
              <button
                key={fav}
                className="sidebar-item"
                onClick={() => onModuleSelect(`__files__:${fav}`)}
                style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}
                title={fav}
              >
                <Star size={14} style={{ color: 'var(--accent,#cdf24b)' }} />
                <span style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {fav.split('/').pop() || fav}
                </span>
              </button>
            ))}
          </div>
        </>
      )}

      <div className="sidebar-footer">
        <button className="sidebar-item" onClick={onNotificationClick} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left', position: 'relative' }}>
          <Bell size={18} />
          {t(locale, 'notifications.title')}
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__store__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <ShoppingBag size={18} />
          {t(locale, 'nav.store')}
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__settings__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <Settings size={18} />
          {t(locale, 'nav.settings')}
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__workshop__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <Layers size={18} />
          {t(locale, 'nav.workshop')}
        </button>
      </div>

      <div
        className={`sidebar-drag-handle ${isDragging ? 'active' : ''}`}
        onMouseDown={handleMouseDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
      />
    </aside>
  );
}
