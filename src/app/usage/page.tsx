'use client';

import { UsageDashboard } from '@/components/dashboard/UsageDashboard';
import '@/types';

/**
 * 数据/用量页（决策 10）：完整 Usage Dashboard 的唯一生产入口。
 * `/` 首页已让位 PersonalWorkspace Home，不再承载完整用量；完整用量归本页。
 */
export default function UsagePage() {
  return (
    <div className="w-full h-full flex flex-col overflow-y-auto">
      <UsageDashboard />
    </div>
  );
}
