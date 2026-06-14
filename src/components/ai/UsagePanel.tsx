'use client';

import { useState } from 'react';

export default function UsagePanel() {
  const [activeTab, setActiveTab] = useState<'claude' | 'codex' | 'rtk'>('claude');

  const tabs = [
    { id: 'claude' as const, label: 'Claude Code' },
    { id: 'codex' as const, label: 'Codex' },
    { id: 'rtk' as const, label: 'RTK' },
  ];

  return (
    <div style={{ padding: 8 }}>
      <div style={{ display: 'flex', gap: 4, marginBottom: 8, borderBottom: '1px solid var(--border)' }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            className="btn btn-ghost"
            onClick={() => setActiveTab(t.id)}
            style={{
              fontSize: 11, padding: '4px 10px', flex: 1, borderRadius: 0,
              borderBottom: activeTab === t.id ? '2px solid var(--accent)' : '2px solid transparent',
              color: activeTab === t.id ? 'var(--accent)' : 'var(--text-dim)',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {activeTab === 'claude' && (
        <div>
          <ProgressBar label="5h Window" used={0} limit={50000} color="#cdf24b" />
          <ProgressBar label="Weekly Quota" used={0} limit={500000} color="#4bcdf2" />
          <div style={{ marginTop: 12, fontSize: 11, color: 'var(--text-dim)' }}>
            <div>Local 5h: 0 tokens</div>
            <div>Today: 0 tokens</div>
            <div>This week: 0 tokens</div>
          </div>
          <button className="btn btn-ghost" style={{ marginTop: 8, fontSize: 11 }}>Refresh</button>
        </div>
      )}

      {activeTab === 'codex' && (
        <div>
          <ProgressBar label="5h Window" used={0} limit={30000} color="#DEA584" />
          <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-dim)' }}>Plan: Free</div>
        </div>
      )}

      {activeTab === 'rtk' && (
        <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>
          <div style={{ marginBottom: 8 }}>
            <span style={{ color: 'var(--text)', fontWeight: 600 }}>0</span> tokens saved
          </div>
          <div style={{ marginBottom: 8 }}>
            <span style={{ color: 'var(--text)', fontWeight: 600 }}>0</span> commands
          </div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
            No RTK usage data yet
          </div>
        </div>
      )}
    </div>
  );
}

function ProgressBar({ label, used, limit, color }: { label: string; used: number; limit: number; color: string }) {
  const pct = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginBottom: 4 }}>
        <span style={{ color: 'var(--text-dim)' }}>{label}</span>
        <span style={{ color: 'var(--text)' }}>{Math.round(pct)}%</span>
      </div>
      <div style={{ height: 6, background: 'var(--bg-3)', borderRadius: 3, overflow: 'hidden' }}>
        <div style={{ width: `${pct}%`, height: '100%', background: color, borderRadius: 3, transition: 'width 0.3s ease' }} />
      </div>
    </div>
  );
}
