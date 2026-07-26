'use client';

import { UsageDashboard } from '@/components/dashboard/UsageDashboard';
import '@/types';

export default function DashboardPage() {
  return (
    <div className="w-full h-full flex flex-col overflow-y-auto">
      <UsageDashboard />
    </div>
  );
}
