'use client';

import { useState } from 'react';

interface WorkshopPageProps {
  onInstall: (source: string) => void;
}

export default function WorkshopPage({ onInstall }: WorkshopPageProps) {
  const [dragOver, setDragOver] = useState(false);

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  };

  const handleDragLeave = () => {
    setDragOver(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);

    const files = Array.from(e.dataTransfer.files);
    for (const file of files) {
      if (file.name.endsWith('.zip') || file.type === '') {
        onInstall(file.path || file.name);
      }
    }
  };

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>
          Workshop
        </h1>
        <p style={{ fontSize: 12, color: 'var(--text-dim,#9b9d8c)', margin: '4px 0 0' }}>
          Browse and manage your modules
        </p>
      </div>

      {/* Drop zone */}
      <div
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        style={{
          margin: '16px 20px',
          padding: 40,
          border: `2px dashed ${dragOver ? 'var(--accent,#cdf24b)' : 'var(--border,#262920)'}`,
          borderRadius: 8,
          textAlign: 'center',
          color: 'var(--text-faint,#62655a)',
          fontSize: 13,
          transition: 'all 0.16s cubic-bezier(0.2,0.7,0.3,1)',
          background: dragOver ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
        }}
      >
        {dragOver ? (
          <span style={{ color: 'var(--accent,#cdf24b)' }}>Release to install module</span>
        ) : (
          <>
            <div style={{ fontSize: 24, marginBottom: 8 }}>📦</div>
            <div>Drag & drop a module directory or .zip file here</div>
          </>
        )}
      </div>

      {/* Empty state */}
      <div style={{
        flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
        color: 'var(--text-faint,#62655a)', fontSize: 13,
      }}>
        <div style={{ textAlign: 'center' }}>
          <p>No modules installed yet</p>
          <p style={{ fontSize: 11, marginTop: 4 }}>Drag a module above to get started</p>
        </div>
      </div>
    </div>
  );
}
