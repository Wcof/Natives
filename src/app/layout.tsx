import type { Metadata } from 'next';
import './globals.css';
import ShellLayout from '@/components/shell/ShellLayout';
import { ToastProvider } from '@/components/ui/Toast';

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
    <html data-theme="editorial">
      <body>
        <ToastProvider>
          <ShellLayout>{children}</ShellLayout>
        </ToastProvider>
      </body>
    </html>
  );
}
