'use client';

export default function StorePage() {
  return (
    <div style={{ height: '100%', overflow: 'auto', padding: '40px 20px', textAlign: 'center' }}>
      <div style={{ fontSize: 44, marginBottom: 16 }}>🛒</div>
      <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', marginBottom: 8 }}>App Store</h1>
      <p style={{ fontSize: 13, color: 'var(--text-faint)', marginBottom: 24, maxWidth: 400, margin: '0 auto 24px' }}>
        Browse and install modules to extend your Natives experience.
      </p>
      <div style={{ display: 'flex', justifyContent: 'center', gap: 8 }}>
        <div className="btn">Browse Modules</div>
      </div>
    </div>
  );
}
