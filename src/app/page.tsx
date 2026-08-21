'use client';

import HomeWorkspacePage from '@/components/home/HomeWorkspacePage';
import '@/types';

/**
 * `/` —— 唯一 Personal Workspace Home（ADR-0020 §3 / 决策 1/10）。
 * 完整 Usage Dashboard 已迁至数据/用量页（/usage），首页不再承载完整用量。
 */
export default function DashboardPage() {
  return (
    <div className="w-full h-full flex flex-col overflow-hidden">
      <HomeWorkspacePage />
    </div>
  );
}
