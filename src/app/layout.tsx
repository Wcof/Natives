import './globals.css';
import { GeistSans, GeistMono } from 'geist/font';
import RootClient from './RootClient';

/* ═══════════════════════════════════════════════
   Root Layout — Server Component
   AI Natives Design System V1.0 — Light/Dark 双主题
   Brand accent #FF6B2C (Primary Orange)
   Font-locked: Geist Sans (display) + Geist Mono (code).
   ═══════════════════════════════════════════════ */

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html
      lang="zh-CN"
      data-theme="dark"
      className={`h-full ${GeistSans.variable} ${GeistMono.variable}`}
      style={{ background: 'var(--background)' }}
      suppressHydrationWarning
    >
      <body className="h-full overflow-hidden antialiased" style={{ background: 'var(--background)', color: 'var(--text)' }}>
        <RootClient>{children}</RootClient>
      </body>
    </html>
  );
}
