'use client';

import { useEffect, useState } from 'react';

interface ModuleItem {
  id: string;
  name: string;
  icon?: string;
}

interface SidebarProps {
  isCollapsed: boolean;
  onToggle: () => void;
  width: number;
  onResize: (width: number) => void;
  activeModuleId?: string;
  onModuleSelect: (moduleId: string) => void;
}

export default function Sidebar({
  isCollapsed,
  onToggle,
  width,
  onResize,
  activeModuleId,
  onModuleSelect,
}: SidebarProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [modules, setModules] = useState<ModuleItem[]>([]);
  const [dragIndex, setDragIndex] = useState<number | null>(null);

  // Load modules on mount
  useEffect(() => {
    async function loadModules() {
      try {
        // Try to get modules from IPC
        const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;
        if (api && typeof (api as Record<string, unknown>).module === 'object') {
          const modApi = (api as Record<string, unknown>).module as { scan?: () => Promise<ModuleItem[]> };
          if (modApi.scan) {
            const result = await modApi.scan();
            if (Array.isArray(result)) {
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

  const handleDragEnd = () => {
    setDragIndex(null);
  };

  return (
    <aside
      className={`sidebar ${isCollapsed ? 'collapsed' : ''}`}
      style={{ width: isCollapsed ? 0 : width }}
      role="navigation"
      aria-label="Module sidebar"
    >
      <div className="sidebar-header">
        <span className="sidebar-brand">NATIVES</span>
        <button className="btn-ghost" onClick={onToggle} aria-label="Toggle sidebar">
          <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M9 18l6-6-6-6" />
          </svg>
        </button>
      </div>

      <div className="sidebar-section-title" id="sidebar-modules-label">Modules</div>
      <div
        className="sidebar-modules"
        role="listbox"
        aria-labelledby="sidebar-modules-label"
        onDragEnd={handleDragEnd}
      >
        {modules.length === 0 ? (
          <div style={{ padding: '20px 12px', textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            No modules installed
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
                  <svg className="icon-md" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  </svg>
                )}
              </span>
              <span className="sidebar-module-name">{mod.name}</span>
            </div>
          ))
        )}
      </div>

      <div className="sidebar-footer">
        <button className="sidebar-item" onClick={() => onModuleSelect('__settings__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <svg className="icon-md" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
          Settings
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__workshop__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <svg className="icon-md" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
          </svg>
          Workshop
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
