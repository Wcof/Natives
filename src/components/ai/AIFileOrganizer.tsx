'use client';

import { useState } from 'react';

interface AIProposal {
  id: string;
  action: 'move' | 'rename' | 'delete' | 'archive';
  filePath: string;
  reason: string;
  targetPath?: string;
}

export default function AIFileOrganizer() {
  const [proposals, setProposals] = useState<AIProposal[]>([]);
  const [approved, setApproved] = useState<Set<string>>(new Set());
  const [executed, setExecuted] = useState(false);

  return (
    <div style={{ padding: 8 }}>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 4, textTransform: 'uppercase' }}>
        AI File Organizer
      </div>
      <div style={{ fontSize: 10, color: 'var(--text-faint)', marginBottom: 12 }}>
        AI analyzes file metadata to suggest organization improvements
      </div>

      {proposals.length === 0 ? (
        <>
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            No suggestions yet
          </div>
          <button className="btn btn-primary" style={{ width: '100%', fontSize: 12 }} onClick={() => {}}>
            Analyze Current Folder
          </button>
        </>
      ) : (
        <>
          {proposals.map((p) => (
            <div key={p.id} style={{
              padding: 8, marginBottom: 6, borderRadius: 'var(--radius)',
              border: `1px solid ${approved.has(p.id) ? 'var(--accent)' : 'var(--border)'}`,
              background: approved.has(p.id) ? 'rgba(205,242,75,0.08)' : 'var(--bg-2)',
            }}>
              <label style={{ display: 'flex', gap: 8, cursor: 'pointer', fontSize: 12 }}>
                <input type="checkbox" checked={approved.has(p.id)} onChange={() => {
                  setApproved((prev) => {
                    const next = new Set(prev);
                    if (next.has(p.id)) next.delete(p.id);
                    else next.add(p.id);
                    return next;
                  });
                }} />
                <div>
                  <div style={{ color: 'var(--text)' }}>
                    {p.action === 'move' ? '📦 Move' : p.action === 'rename' ? '✏️ Rename' : p.action === 'delete' ? '🗑️ Delete' : '📁 Archive'}
                    {' '}{p.filePath.split('/').pop()}
                  </div>
                  <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 2 }}>{p.reason}</div>
                </div>
              </label>
            </div>
          ))}

          <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
            <button className="btn btn-primary" style={{ flex: 1, fontSize: 11 }} disabled={approved.size === 0 || executed}>
              Execute ({approved.size})
            </button>
            <button className="btn btn-ghost" style={{ fontSize: 11 }} onClick={() => { setProposals([]); setApproved(new Set()); setExecuted(false); }}>
              Undo All
            </button>
          </div>
        </>
      )}
    </div>
  );
}
