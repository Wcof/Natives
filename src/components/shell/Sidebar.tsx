'use client';

import { useEffect, useState } from 'react';
import { Settings, Layers, ChevronRight, Square, ShoppingBag } from 'lucide-react';

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
          <ChevronRight size={14} />
        </button>
      </div>

      <div className="sidebar-section-title" id="sidebar-modules-label">Modules</div>
      <div
        className="sidebar-modules"
        role="listbox"
        aria-labelledby="sidebar-modules-label"
        aria-live="polite"
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
                  <Square size={18} />
                )}
              </span>
              <span className="sidebar-module-name">{mod.name}</span>
            </div>
          ))
        )}
      </div>

      <div className="sidebar-footer">
        <button className="sidebar-item" onClick={() => onModuleSelect('__store__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <ShoppingBag size={18} />
          Store
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__settings__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <Settings size={18} />
          Settings
        </button>
        <button className="sidebar-item" onClick={() => onModuleSelect('__workshop__')} style={{ width: '100%', border: 'none', background: 'none', cursor: 'pointer', textAlign: 'left' }}>
          <Layers size={18} />
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
