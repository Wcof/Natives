'use client';

export default function DashboardPage() {
  return (
    <div style={{ padding: '60px 20px', textAlign: 'center' }}>
      <h1 style={{ fontSize: 32, fontWeight: 300, color: 'var(--text)', marginBottom: 8 }}>
        Natives
      </h1>
      <p style={{ color: 'var(--text-faint)', fontSize: 14, marginBottom: 32 }}>
        AI Steam Base
      </p>
      <div style={{ display: 'flex', justifyContent: 'center', gap: 12 }}>
        <button className="btn btn-primary">Open Terminal</button>
        <button className="btn">Install Module</button>
        <button className="btn">Settings</button>
      </div>
    </div>
  );
}
