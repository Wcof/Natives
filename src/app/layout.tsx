import type { Metadata } from 'next';
import './globals.css';
import ShellLayout from '@/components/shell/ShellLayout';

export const metadata: Metadata = {
  title: 'Natives',
  description: 'AI Steam Base — 桌面应用容器',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="zh-CN" data-theme="terminal-volt">
      <body>
        <ShellLayout>{children}</ShellLayout>
      </body>
    </html>
  );
}
