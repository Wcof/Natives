'use client';

import { useState } from 'react';

interface DashboardCard {
  title: string;
  icon: string;
  description: string;
  onClick: () => void;
}

export default function Dashboard() {
  const cards: DashboardCard[] = [
    { title: 'File Browser', icon: '📁', description: 'Browse and manage files', onClick: () => {} },
    { title: 'AI Workbench', icon: '🤖', description: 'Agent sessions and tools', onClick: () => {} },
    { title: 'Terminal', icon: '⌨️', description: 'Command line with env injection', onClick: () => {} },
    { title: 'Workshop', icon: '🧩', description: 'Browse and install plugins', onClick: () => {} },
    { title: 'Settings', icon: '⚙️', description: 'Configure themes and profiles', onClick: () => {} },
  ];

  return (
    <div style={{ padding: 24, height: '100%', overflow: 'auto' }}>
      <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--text)', marginBottom: 4 }}>
        Natives
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 24 }}>
        AI-native application container
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))', gap: 12 }}>
        {cards.map((card) => (
          <button
            key={card.title}
            onClick={card.onClick}
            style={{
              padding: 16, textAlign: 'left', cursor: 'pointer',
              background: 'var(--bg-2)', border: '1px solid var(--border)',
              borderRadius: 'var(--radius)', transition: 'all 0.12s',
            }}
            onMouseEnter={(e) => { e.currentTarget.style.borderColor = 'var(--accent)'; e.currentTarget.style.background = 'var(--bg-3)'; }}
            onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'var(--border)'; e.currentTarget.style.background = 'var(--bg-2)'; }}
          >
            <div style={{ fontSize: 28, marginBottom: 8 }}>{card.icon}</div>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>{card.title}</div>
            <div style={{ fontSize: 11, color: 'var(--text-faint)' }}>{card.description}</div>
          </button>
        ))}
      </div>
    </div>
  );
}
