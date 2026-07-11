// ─── Run Inspector ───────────────────────────────────────
//
// Right panel showing run details, events, artifacts, and sub-agent state.

import { useState } from 'react';
import { Play, Square, CheckCircle, XCircle, AlertTriangle, Clock, FileText, GitBranch } from 'lucide-react';

interface RunEvent {
  sequence: number;
  type: string;
  timestamp: string;
  summary?: string;
}

interface Artifact {
  id: string;
  label?: string;
  path: string;
  kind: string;
  size: number;
}

interface RunInspectorProps {
  runId: string | null;
  status: string;
  events: RunEvent[];
  artifacts: Artifact[];
  subAgents: Array<{ id: string; task: string; status: string }>;
  locale: string;
}

export default function RunInspector({
  runId, status, events, artifacts, subAgents, locale,
}: RunInspectorProps) {
  const [activeTab, setActiveTab] = useState<'events' | 'artifacts' | 'subagents'>('events');

  if (!runId) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-disabled)]">
        {locale.startsWith('zh') ? '选择运行以查看详情' : 'Select a run to inspect'}
      </div>
    );
  }

  const statusIcon = {
    queued: Clock, preparing: Clock, running: Play,
    waiting_permission: AlertTriangle, cancelling: Square,
    completed: CheckCircle, failed: XCircle, interrupted: Square,
  }[status] || Clock;

  const StatusIcon = statusIcon;

  const tabs = [
    { id: 'events' as const, label: locale.startsWith('zh') ? '事件' : 'Events', icon: GitBranch },
    { id: 'artifacts' as const, label: locale.startsWith('zh') ? '产物' : 'Artifacts', icon: FileText },
    { id: 'subagents' as const, label: locale.startsWith('zh') ? '子任务' : 'Sub-agents', icon: GitBranch },
  ];

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-4 py-3 border-b border-[var(--border)]">
        <div className="flex items-center gap-2">
          <StatusIcon size={14} />
          <span className="text-sm font-medium truncate font-mono">{runId.slice(0, 8)}</span>
          <span className="text-xs px-1.5 py-0.5 rounded bg-[var(--surface-hover)] text-[var(--text-secondary)]">
            {status}
          </span>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-[var(--border)]">
        {tabs.map(tab => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-1.5 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === tab.id
                ? 'text-[var(--primary)] border-b-2 border-[var(--primary)]'
                : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
            }`}
          >
            <tab.icon size={12} />
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-3 space-y-1">
        {activeTab === 'events' && (
          events.length === 0
            ? <EmptyState message={locale.startsWith('zh') ? '暂无事件' : 'No events'} />
            : events.map(event => (
                <div key={event.sequence} className="flex items-start gap-2 px-2 py-1.5 text-xs rounded hover:bg-[var(--surface-hover)]">
                  <span className="text-[var(--text-disabled)] font-mono w-6 shrink-0">#{event.sequence}</span>
                  <span className="text-[var(--text-secondary)] font-medium">{event.type}</span>
                  {event.summary && <span className="text-[var(--text-disabled)] truncate">{event.summary}</span>}
                </div>
              ))
        )}

        {activeTab === 'artifacts' && (
          artifacts.length === 0
            ? <EmptyState message={locale.startsWith('zh') ? '暂无产物' : 'No artifacts'} />
            : artifacts.map(art => (
                <div key={art.id} className="flex items-center gap-2 px-2 py-1.5 text-xs rounded hover:bg-[var(--surface-hover)]">
                  <FileText size={12} className="text-[var(--text-disabled)]" />
                  <span className="truncate flex-1">{art.label || art.path}</span>
                  <span className="text-[var(--text-disabled)]">{(art.size / 1024).toFixed(1)} KB</span>
                </div>
              ))
        )}

        {activeTab === 'subagents' && (
          subAgents.length === 0
            ? <EmptyState message={locale.startsWith('zh') ? '暂无子任务' : 'No sub-agents'} />
            : subAgents.map(sa => (
                <div key={sa.id} className="flex items-center gap-2 px-2 py-1.5 text-xs rounded hover:bg-[var(--surface-hover)]">
                  <GitBranch size={12} className="text-[var(--text-disabled)]" />
                  <span className="truncate flex-1">{sa.task}</span>
                  <span className="text-[var(--text-disabled)]">{sa.status}</span>
                </div>
              ))
        )}
      </div>
    </div>
  );
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex items-center justify-center h-24 text-xs text-[var(--text-disabled)]">
      {message}
    </div>
  );
}