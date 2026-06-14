'use client';

import { useState, useEffect } from 'react';
import { type FileChangeEvent } from '@/types/agent';

export default function AgentDashboard({ sessionId }: { sessionId?: string }) {
  const [changes, setChanges] = useState<FileChangeEvent[]>([]);

  // Simulate receiving file change events
  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel, data: any) => {
      if (channel === 'file:changed' && data?.path) {
        setChanges((prev) => [{ path: data.path, type: data.type || 'modify', timestamp: Date.now(), sessionId }, ...prev].slice(0, 50));
      }
    });
    return unsub;
  }, [sessionId]);

  const getIntensity = (idx: number) => Math.max(0.2, 1 - idx * 0.04);

  return (
    <div style={{ padding: 8 }}>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
        Agent Changes
      </div>
      {changes.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          Waiting for agent changes...
        </div>
      ) : (
        changes.map((ch, i) => (
          <div
            key={`${ch.path}-${ch.timestamp}-${i}`}
            style={{
              padding: '6px 8px',
              marginBottom: 4,
              borderRadius: 'var(--radius)',
              fontSize: 12,
              background: `rgba(205, 242, 75, ${getIntensity(i) * 0.15})`,
              borderLeft: `3px solid rgba(205, 242, 75, ${getIntensity(i)})`,
              animation: i === 0 ? 'livePulse 1.1s ease-in-out infinite' : undefined,
              transition: 'opacity 0.3s',
              color: 'var(--text)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            <span style={{ fontSize: 10, marginRight: 4 }}>
              {ch.type === 'create' ? '+' : ch.type === 'delete' ? '−' : '✎'}
            </span>
            {ch.path.split('/').pop()}
          </div>
        ))
      )}
    </div>
  );
}
