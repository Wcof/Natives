'use client';

import { useState } from 'react';
import { type FileChangeEvent } from '@/types/agent';

export default function ChangeInbox() {
  const [items, setItems] = useState<(FileChangeEvent & { project?: string })[]>([]);

  const groupedByProject = items.reduce<Record<string, typeof items>>((acc, item) => {
    const project = item.project || 'Unknown';
    if (!acc[project]) acc[project] = [];
    acc[project]!.push(item);
    return acc;
  }, {});

  return (
    <div style={{ padding: 8 }}>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8, textTransform: 'uppercase' }}>
        Change Inbox ({items.length})
      </div>

      {items.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          No changes yet
        </div>
      ) : (
        Object.entries(groupedByProject).map(([project, changes]) => (
          <div key={project} style={{ marginBottom: 12 }}>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              {project}
            </div>
            {changes.map((ch, i) => (
              <div key={`${ch.path}-${i}`} style={{
                padding: '4px 8px', fontSize: 12, color: 'var(--text-dim)',
                borderBottom: '1px solid var(--border)',
              }}>
                <span style={{ marginRight: 4 }}>
                  {ch.type === 'create' ? '🟢' : ch.type === 'delete' ? '🔴' : '🟡'}
                </span>
                {ch.path.split('/').pop()}
              </div>
            ))}
          </div>
        ))
      )}
    </div>
  );
}
