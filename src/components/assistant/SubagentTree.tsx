'use client';

import { GitBranch, Loader2, CheckCircle, XCircle } from 'lucide-react';

interface SubAgent {
  id: string;
  task: string;
  status: 'queued' | 'running' | 'completed' | 'failed';
  depth: number;
  createdAt: string;
}

interface SubagentTreeProps {
  subAgents: SubAgent[];
  onSelect?: (id: string) => void;
  locale: string;
}

export default function SubagentTree({ subAgents, onSelect, locale }: SubagentTreeProps) {
  if (subAgents.length === 0) {
    return (
      <div className="flex items-center justify-center h-20 text-xs text-[var(--text-disabled)]">
        {locale.startsWith('zh') ? '无子任务' : 'No sub-agents'}
      </div>
    );
  }

  const statusIcon = (status: string) => {
    switch (status) {
      case 'queued': return <Loader2 size={12} className="text-[var(--text-disabled)]" />;
      case 'running': return <Loader2 size={12} className="text-blue-500 animate-spin" />;
      case 'completed': return <CheckCircle size={12} className="text-green-500" />;
      case 'failed': return <XCircle size={12} className="text-red-500" />;
      default: return <GitBranch size={12} className="text-[var(--text-disabled)]" />;
    }
  };

  return (
    <div className="space-y-1">
      {subAgents.map((agent) => (
        <button
          key={agent.id}
          onClick={() => onSelect?.(agent.id)}
          className="w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-left text-xs hover:bg-[var(--surface-hover)] transition-colors"
        >
          {statusIcon(agent.status)}
          <div className="flex-1 min-w-0">
            <div className="truncate text-[var(--text-secondary)]">{agent.task}</div>
            <div className="text-[var(--text-disabled)] mt-0.5">
              {agent.status}
              {agent.depth > 0 && ` · depth ${agent.depth}`}
            </div>
          </div>
        </button>
      ))}
    </div>
  );
}