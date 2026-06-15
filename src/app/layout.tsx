import type { Metadata } from 'next';
import './globals.css';
import ShellLayout from '@/components/shell/ShellLayout';

export const metadata: Metadata = {
  title: 'Natives',
  description: 'Natives — AI Steam Base',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html data-theme="terminal-volt">
      <body>
        <ShellLayout>{children}</ShellLayout>
      </body>
    </html>
  );
}
