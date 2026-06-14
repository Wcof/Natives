'use client';

import { useState } from 'react';
import { type SkillInfo } from '@/types/agent';

export default function SkillsPanel() {
  const [skills] = useState<SkillInfo[]>([]);
  const [filter, setFilter] = useState<'all' | 'healthy' | 'issues'>('all');

  const filtered = filter === 'all' ? skills : skills.filter((s) => filter === 'healthy' ? s.health.ok : !s.health.ok);

  return (
    <div style={{ padding: 8 }}>
      {/* Filter tabs */}
      <div style={{ display: 'flex', gap: 4, marginBottom: 8 }}>
        {(['all', 'healthy', 'issues'] as const).map((f) => (
          <button
            key={f}
            className={`btn btn-ghost`}
            onClick={() => setFilter(f)}
            style={{
              fontSize: 10, padding: '2px 8px',
              color: filter === f ? 'var(--accent)' : 'var(--text-dim)',
              borderBottom: filter === f ? '2px solid var(--accent)' : '2px solid transparent',
              borderRadius: 0,
            }}
          >
            {f}
          </button>
        ))}
      </div>

      {filtered.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          No skills installed
        </div>
      ) : (
        filtered.map((skill) => (
          <div key={skill.name} style={{
            padding: '8px', marginBottom: 4,
            borderRadius: 'var(--radius)',
            border: '1px solid var(--border)',
            background: 'var(--bg-2)',
          }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
              <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{skill.name}</span>
              <span style={{
                width: 8, height: 8, borderRadius: '50%',
                background: skill.health.ok ? '#4bcdf2' : '#f24b4b',
              }} />
            </div>
            <div style={{ fontSize: 10, color: 'var(--text-faint)', marginBottom: 2 }}>
              {skill.description?.slice(0, 100)}{skill.description?.length > 100 ? '...' : ''}
            </div>
            <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>
              Triggered {skill.triggerCount}x · {skill.source}
            </div>
          </div>
        ))
      )}
    </div>
  );
}
