'use client';

import { useState } from 'react';
import { type AgentSession } from '@/types/agent';

export default function ProjectMemory() {
  const [sessions] = useState<AgentSession[]>([]);
  const [selected, setSelected] = useState<string | null>(null);

  return (
    <div style={{ padding: 8 }}>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8, textTransform: 'uppercase' }}>
        Project Memory ({sessions.length} sessions)
      </div>

      {sessions.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          No sessions found. Start working with an AI agent.
        </div>
      ) : (
        sessions.map((s) => (
          <div
            key={s.id}
            onClick={() => setSelected(s.id)}
            style={{
              padding: '8px', marginBottom: 4,
              borderRadius: 'var(--radius)',
              cursor: 'pointer',
              background: selected === s.id ? 'var(--bg-3)' : 'transparent',
              border: '1px solid var(--border)',
            }}
          >
            <div style={{ fontSize: 12, color: 'var(--text)', marginBottom: 4, fontWeight: 600 }}>
              {s.title}
            </div>
            <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
              {s.engine} · {s.filesModified.length} files · {new Date(s.startTime).toLocaleDateString()}
            </div>
            {selected === s.id && (
              <div style={{ marginTop: 6 }}>
                {s.filesModified.length > 0 && (
                  <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 4 }}>
                    Files: {s.filesModified.slice(0, 5).join(', ')}
                  </div>
                )}
                {s.skillsUsed.length > 0 && (
                  <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                    Skills: {s.skillsUsed.join(', ')}
                  </div>
                )}
                <button className="btn btn-primary" style={{ marginTop: 6, fontSize: 11, padding: '4px 10px' }}>
                  Restore Session
                </button>
              </div>
            )}
          </div>
        ))
      )}
    </div>
  );
}
