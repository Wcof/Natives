import './globals.css';
import RootClient from './RootClient';

/* ═══════════════════════════════════════════════
   Root Layout — Server Component
   Global CSS import must live in a Server Component
   to prevent Next.js from treating it as a CSS module.
   ═══════════════════════════════════════════════ */

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="zh-CN" data-theme="terminal-volt" className="h-full bg-transparent">
      <body className="h-full overflow-hidden bg-transparent text-content-text antialiased">
        <RootClient>{children}</RootClient>
      </body>
    </html>
  );
}
