'use client';

import { ReactNode } from 'react';

interface ContentAreaProps {
  children?: ReactNode;
}

export default function ContentArea({ children }: ContentAreaProps) {
  return (
    <main className="content-area">
      {children || (
        <div style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          color: 'var(--text-faint)',
          fontSize: '24px',
          fontWeight: 300,
        }}>
          Select a module to get started
        </div>
      )}
    </main>
  );
}
