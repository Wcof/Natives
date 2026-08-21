import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import 'react-grid-layout/css/styles.css';
import 'react-resizable/css/styles.css';
import './styles.css';

export const metadata: Metadata = {
  title: 'HOME-P0 Grid Spike',
};

export default function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
