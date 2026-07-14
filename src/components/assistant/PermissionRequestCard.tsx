'use client';

import { useState } from 'react';
import { Shield, Check, X, Clock, AlertTriangle } from 'lucide-react';

interface PermissionRequest {
  id: string;
  toolName: string;
  reason: string;
  input: Record<string, unknown>;
  status: 'pending' | 'approved' | 'rejected' | 'expired';
  createdAt: string;
}

interface PermissionRequestCardProps {
  request: PermissionRequest;
  onApprove: (id: string, scope: 'once' | 'this_run' | 'project') => void;
  onReject: (id: string) => void;
  locale: string;
}

export default function PermissionRequestCard({
  request, onApprove, onReject, locale,
}: PermissionRequestCardProps) {
  const [scope, setScope] = useState<'once' | 'this_run' | 'project'>('once');

  if (request.status !== 'pending') {
    return null;
  }

  return (
    <div className="border border-yellow-400/30 rounded-xl overflow-hidden bg-yellow-50/50 dark:bg-yellow-950/10 my-3">
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-yellow-400/20 bg-yellow-50/80 dark:bg-yellow-950/20">
        <Shield size={14} className="text-yellow-600 dark:text-yellow-400" />
        <span className="text-xs font-semibold text-yellow-700 dark:text-yellow-300">
          {locale.startsWith('zh') ? '权限请求' : 'Permission Request'}
        </span>
      </div>

      {/* Content */}
      <div className="px-4 py-3 space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono font-medium text-[var(--text-primary)]">
            {request.toolName}
          </span>
        </div>
        <p className="text-xs text-[var(--text-secondary)]">{request.reason}</p>

        {/* Input preview */}
        {request.input && (
          <div className="bg-[var(--surface)] rounded-lg p-2 text-xs font-mono text-[var(--text-secondary)] overflow-x-auto">
            <pre className="whitespace-pre-wrap">{JSON.stringify(request.input, null, 2)}</pre>
          </div>
        )}

        {/* Scope selector */}
        <div className="flex items-center gap-2">
          <span className="text-xs text-[var(--text-disabled)]">
            {locale.startsWith('zh') ? '授权范围:' : 'Scope:'}
          </span>
          {(['once', 'this_run', 'project'] as const).map((s) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={`px-2 py-0.5 rounded text-xs transition-colors ${
                scope === s
                  ? 'bg-[var(--accent)] text-[var(--accent-ink)]'
                  : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
              }`}
            >
              {s === 'once' ? (locale.startsWith('zh') ? '一次' : 'Once') :
               s === 'this_run' ? (locale.startsWith('zh') ? '本次运行' : 'This Run') :
               (locale.startsWith('zh') ? '项目' : 'Project')}
            </button>
          ))}
        </div>
      </div>

      {/* Actions */}
      <div className="flex gap-2 px-4 py-2.5 border-t border-yellow-400/20">
        <button
          onClick={() => onReject(request.id)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-red-100 dark:bg-red-950/30 text-red-600 dark:text-red-400 hover:bg-red-200 dark:hover:bg-red-950/50 transition-colors"
        >
          <X size={12} />
          {locale.startsWith('zh') ? '拒绝' : 'Reject'}
        </button>
        <button
          onClick={() => onApprove(request.id, scope)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-green-100 dark:bg-green-950/30 text-green-600 dark:text-green-400 hover:bg-green-200 dark:hover:bg-green-950/50 transition-colors ml-auto"
        >
          <Check size={12} />
          {locale.startsWith('zh') ? '允许' : 'Allow'}
        </button>
      </div>
    </div>
  );
}