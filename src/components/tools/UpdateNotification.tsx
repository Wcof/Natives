'use client';

import { useState } from 'react';

interface VersionInfo {
  latest: string;
  current: string;
  releaseNotes: string;
}

export default function UpdateNotification({ onClose }: { onClose: () => void }) {
  const [version] = useState<VersionInfo | null>(null);
  const [dismissed, setDismissed] = useState(false);

  if (dismissed || !version) return null;

  return (
    <div style={{
      position: 'fixed', bottom: 20, right: 20, zIndex: 1000,
      padding: '12px 16px', background: 'var(--panel)', borderRadius: 'var(--radius)',
      border: '1px solid var(--accent)', maxWidth: 320,
      boxShadow: '0 4px 20px rgba(0,0,0,0.3)',
    }}>
      <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--accent)', marginBottom: 4 }}>
        Update Available
      </div>
      <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 8 }}>
        v{version.current} → v{version.latest}
      </div>
      <div style={{ display: 'flex', gap: 6 }}>
        <a className="btn btn-primary" style={{ fontSize: 11, padding: '4px 10px', textDecoration: 'none' }}
          href={`https://github.com/Wcof/Natives/releases/latest`} target="_blank">
          Download
        </a>
        <button className="btn btn-ghost" style={{ fontSize: 11 }} onClick={() => setDismissed(true)}>Dismiss</button>
      </div>
    </div>
  );
}
