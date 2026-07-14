'use client';

import React from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import { UsageDashboard } from '@/components/dashboard/UsageDashboard';
import '@/types';

export default function DashboardPage() {
  return (
    <div className="w-full h-full flex flex-col overflow-y-auto">
      <UsageDashboard />
    </div>
  );
}
