'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

interface TerminalPanelProps {
  isCollapsed: boolean;
  onToggle: () => void;
  height: number;
  onResize: (height: number) => void;
  isMaximized: boolean;
  onMaximizeToggle: () => void;
}

export default function TerminalPanel({
  isCollapsed,
  onToggle,
  height,
  onResize,
  isMaximized,
  onMaximizeToggle,
}: TerminalPanelProps) {
  const [isDragging, setIsDragging] = useState(false);
  const dragRef = useRef(false);
  const startY = useRef(0);
  const startHeight = useRef(0);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragRef.current = true;
    setIsDragging(true);
    startY.current = e.clientY;
    startHeight.current = height;
  }, [height]);

  useEffect(() => {
    if (!isDragging) return;
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const delta = startY.current - e.clientY;
      const newHeight = Math.max(200, Math.min(window.innerHeight * 0.5, startHeight.current + delta));
      onResize(newHeight);
    };
    const handleMouseUp = () => {
      dragRef.current = false;
      setIsDragging(false);
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isDragging, onResize]);

  return (
    <div
      className={`terminal-panel ${isCollapsed ? 'collapsed' : ''} ${isMaximized ? 'terminal-maximized' : ''}`}
      style={{ height: isMaximized ? '100%' : isCollapsed ? 0 : height }}
      role="region"
      aria-label="Terminal"
    >
      {/* Drag handle */}
      <div
        className={`terminal-drag-handle ${isDragging ? 'active' : ''}`}
        onMouseDown={handleMouseDown}
      />

      <div className="terminal-header">
        <div className="terminal-tabs">
          <div className="terminal-tab active">zsh</div>
        </div>
        <div className="terminal-actions">
          <button className="btn-ghost" onClick={onMaximizeToggle} aria-label={isMaximized ? 'Restore terminal' : 'Maximize terminal'}>
            <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              {isMaximized ? (
                <path d="M8 3v3a2 2 0 01-2 2H3m18 0h-3a2 2 0 01-2-2V3m0 18v-3a2 2 0 012-2h3M3 16h3a2 2 0 012 2v3" />
              ) : (
                <path d="M8 3H5a2 2 0 00-2 2v3m18 0V5a2 2 0 00-2-2h-3m0 18h3a2 2 0 002-2v-3M3 16v3a2 2 0 002 2h3" />
              )}
            </svg>
          </button>
          <button className="btn-ghost" onClick={onToggle} aria-label={isCollapsed ? 'Open terminal' : 'Close terminal'}>
            <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              {isCollapsed ? (
                <path d="M18 15l-6-6-6 6" />
              ) : (
                <path d="M6 9l6 6 6-6" />
              )}
            </svg>
          </button>
        </div>
      </div>

      <div className="terminal-body" id="terminal-container" />
    </div>
  );
}
