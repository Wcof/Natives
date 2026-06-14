'use client';

import { ReactNode } from 'react';

interface RightPanelProps {
  isOpen: boolean;
  onToggle: () => void;
  title?: string;
  children?: ReactNode;
}

export default function RightPanel({ isOpen, onToggle, title = 'Panel', children }: RightPanelProps) {
  return (
    <aside
      className={`right-panel ${!isOpen ? 'collapsed' : ''}`}
      role="region"
      aria-label={title}
    >
      <div className="right-panel-header">
        <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim)' }}>
          {title}
        </span>
        <button className="btn-ghost" onClick={onToggle} aria-label="Close panel">
          <svg className="icon-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
      <div className="right-panel-content">
        {children || (
          <p style={{ color: 'var(--text-faint)', fontSize: 13 }}>No content</p>
        )}
      </div>
    </aside>
  );
}
