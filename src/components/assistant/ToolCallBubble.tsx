'use client';

import { useState } from 'react';

interface ToolCallBubbleProps {
  toolName: string;
  params: string;
  status: 'pending' | 'result' | 'error' | 'circuit_broken';
  result?: string;
  error?: string;
  contractId?: string;
  writePath?: string;
  onRetry?: () => void;
  /** 执行引擎：自愈失败计数 */
  selfHealCount?: number;
  /** 执行引擎：自愈上限 */
  maxSelfHeal?: number;
}

export default function ToolCallBubble({
  toolName,
  params,
  status,
  result,
  error,
  contractId,
  writePath,
  onRetry,
  selfHealCount,
  maxSelfHeal = 3,
}: ToolCallBubbleProps) {
  const [expanded, setExpanded] = useState(false);
  const [resultExpanded, setResultExpanded] = useState(false);

  const statusConfig = {
    pending: {
      bg: 'bg-blue-500/5',
      border: 'border-blue-500/20',
      icon: (
        <div className="w-4 h-4 rounded-full border-2 border-blue-400 border-t-transparent animate-spin" />
      ),
      label: 'Pending',
    },
    result: {
      bg: 'bg-green-500/5',
      border: 'border-green-500/20',
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-green-400">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ),
      label: 'Success',
    },
    error: {
      bg: 'bg-red-500/5',
      border: 'border-red-500/20',
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-red-400">
          <circle cx="12" cy="12" r="10" />
          <line x1="15" y1="9" x2="9" y2="15" />
          <line x1="9" y1="9" x2="15" y2="15" />
        </svg>
      ),
      label: selfHealCount ? `Error (${selfHealCount}/${maxSelfHeal})` : 'Error',
    },
    circuit_broken: {
      bg: 'bg-red-500/10',
      border: 'border-red-500/40',
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="text-red-500">
          <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
          <line x1="12" y1="9" x2="12" y2="13" />
          <line x1="12" y1="17" x2="12.01" y2="17" />
        </svg>
      ),
      label: `Circuit Broken (${selfHealCount ?? maxSelfHeal + 1}/${maxSelfHeal})`,
    },
  };

  const config = statusConfig[status];

  return (
    <div className={`rounded-lg border ${config.bg} ${config.border} p-3 text-sm`}>
      <div className="flex items-center gap-2">
        {config.icon}
        <span className="font-medium text-[var(--vibe-brand-text)]">{toolName}</span>
        <span className={`text-[0.625rem] px-1.5 py-0.5 rounded ${
          status === 'pending' ? 'bg-blue-500/10 text-blue-400' :
          status === 'result' ? 'bg-green-500/10 text-green-400' :
          status === 'circuit_broken' ? 'bg-red-500/20 text-red-500 font-semibold' :
          'bg-red-500/10 text-red-400'
        }`}>
          {config.label}
        </span>
      </div>

      <div className="mt-2 text-xs text-[var(--text-dim)] font-mono truncate">
        {params}
      </div>

      {status === 'pending' && (
        <div className="mt-2 flex gap-1">
          <div className="w-2 h-2 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '0ms' }} />
          <div className="w-2 h-2 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '150ms' }} />
          <div className="w-2 h-2 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '300ms' }} />
        </div>
      )}

      {status === 'result' && (
        <div className="mt-2 space-y-1">
          {writePath && (
            <div className="text-[0.625rem] text-green-400">
              <span className="font-medium">Path:</span> {writePath}
            </div>
          )}
          {contractId && (
            <div className="text-[0.625rem] text-green-400">
              <span className="font-medium">Contract ID:</span> {contractId.slice(0, 16)}...
            </div>
          )}
          {result && (
            <button
              onClick={() => setResultExpanded(!resultExpanded)}
              className="text-[0.625rem] text-[var(--text-faint)] hover:text-[var(--text-dim)] transition-colors"
            >
              {resultExpanded ? 'Collapse' : 'Expand'} result
            </button>
          )}
          {resultExpanded && result && (
            <pre className="text-[0.625rem] text-[var(--text-dim)] bg-[var(--vibe-btn-bg)] rounded p-2 overflow-x-auto mt-1">
              {result}
            </pre>
          )}
        </div>
      )}

      {(status === 'error' || status === 'circuit_broken') && (
        <div className="mt-2 space-y-2">
          <div className={`text-[0.625rem] rounded p-2 ${
            status === 'circuit_broken'
              ? 'text-red-500 bg-red-500/10 border border-red-500/30'
              : 'text-red-400 bg-red-500/5'
          }`}>
            {status === 'circuit_broken'
              ? `Circuit broken after ${selfHealCount ?? maxSelfHeal + 1} failed attempts. Control returned to user.`
              : (error || 'Unknown error')}
          </div>
          {status === 'error' && onRetry && (
            <button
              onClick={onRetry}
              className="text-[0.625rem] text-red-400 hover:text-red-300 underline transition-colors"
            >
              Retry
            </button>
          )}
        </div>
      )}
    </div>
  );
}
