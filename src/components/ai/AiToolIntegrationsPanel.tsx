'use client';

import { useState, useEffect, useCallback } from 'react';
import {
  integrationsApi,
  type DetectResult,
  type InspectResult,
  type PlannedPatch,
  type VerifyResult,
} from '@/lib/tauri/integrations';
import { useLocale, t } from '@/i18n';
import {
  Wrench,
  CheckCircle2,
  AlertCircle,
  RotateCcw,
  FileCode,
  Shield,
  RefreshCw,
  Terminal,
  FileCheck,
  PlayCircle,
} from 'lucide-react';

interface ToolDef {
  id: string;
  name: string;
  descKey: 'toolIntegrations.claudeDesc' | 'toolIntegrations.codexDesc' | 'toolIntegrations.geminiDesc' | 'toolIntegrations.opencodeDesc';
  binary: string;
}

const TOOLS: ToolDef[] = [
  { id: 'claude_code', name: 'Claude Code', descKey: 'toolIntegrations.claudeDesc', binary: 'claude' },
  { id: 'codex', name: 'Codex CLI', descKey: 'toolIntegrations.codexDesc', binary: 'codex' },
  { id: 'gemini_cli', name: 'Gemini CLI', descKey: 'toolIntegrations.geminiDesc', binary: 'gemini' },
  { id: 'opencode', name: 'OpenCode', descKey: 'toolIntegrations.opencodeDesc', binary: 'opencode' },
];

export default function AiToolIntegrationsPanel() {
  const locale = useLocale();
  const [detectResults, setDetectResults] = useState<Record<string, DetectResult>>({});
  const [inspectResults, setInspectResults] = useState<Record<string, InspectResult>>({});
  const [plannedPatches, setPlannedPatches] = useState<Record<string, PlannedPatch>>({});
  const [_verifyResults, setVerifyResults] = useState<Record<string, VerifyResult>>({});
  const [actionLogs, setActionLogs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);

  const runDetectAll = useCallback(async () => {
    setLoading(true);
    for (const tool of TOOLS) {
      try {
        const d = await integrationsApi.detect(tool.id);
        setDetectResults((prev) => ({ ...prev, [tool.id]: d }));
        const ins = await integrationsApi.inspect(tool.id);
        setInspectResults((prev) => ({ ...prev, [tool.id]: ins }));
      } catch (err) {
        console.error(`Failed to scan tool ${tool.id}:`, err);
      }
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void runDetectAll();
  }, [runDetectAll]);

  const handleBackup = async (toolId: string) => {
    try {
      const res = await integrationsApi.backup(toolId);
      setActionLogs((prev) => ({
        ...prev,
        [toolId]: res.created ? `${res.backupPath || 'backups/'}` : t(locale, 'toolIntegrations.noBackupNeeded'),
      }));
    } catch (err) {
      setActionLogs((prev) => ({ ...prev, [toolId]: String(err) }));
    }
  };

  const handlePlan = async (toolId: string) => {
    try {
      const patch = await integrationsApi.plan({ tool: toolId });
      setPlannedPatches((prev) => ({ ...prev, [toolId]: patch }));
      setActionLogs((prev) => ({ ...prev, [toolId]: patch.summary }));
    } catch (err) {
      setActionLogs((prev) => ({ ...prev, [toolId]: String(err) }));
    }
  };

  const handleApply = async (toolId: string) => {
    const patch = plannedPatches[toolId];
    if (!patch) {
      setActionLogs((prev) => ({ ...prev, [toolId]: t(locale, 'toolIntegrations.planFirst') }));
      return;
    }
    try {
      await integrationsApi.backup(toolId);
      await integrationsApi.apply({
        tool: toolId,
        patch,
        userApproved: true,
      });
      const v = await integrationsApi.verify(toolId);
      setVerifyResults((prev) => ({ ...prev, [toolId]: v }));
      const ins = await integrationsApi.inspect(toolId);
      setInspectResults((prev) => ({ ...prev, [toolId]: ins }));
      setActionLogs((prev) => ({
        ...prev,
        [toolId]: v.ok ? t(locale, 'toolIntegrations.applySuccess') : `${t(locale, 'toolIntegrations.verifySuccess')}: ${v.error || t(locale, 'toolIntegrations.unknownError')}`,
      }));
    } catch (err) {
      setActionLogs((prev) => ({ ...prev, [toolId]: String(err) }));
    }
  };

  const handleVerify = async (toolId: string) => {
    try {
      const v = await integrationsApi.verify(toolId);
      setVerifyResults((prev) => ({ ...prev, [toolId]: v }));
      setActionLogs((prev) => ({
        ...prev,
        [toolId]: v.ok ? t(locale, 'toolIntegrations.verifySuccess') : `${v.error || t(locale, 'toolIntegrations.unknownError')}`,
      }));
    } catch (err) {
      setActionLogs((prev) => ({ ...prev, [toolId]: String(err) }));
    }
  };

  const handleRollback = async (toolId: string) => {
    try {
      const res = await integrationsApi.rollback({ tool: toolId });
      const ins = await integrationsApi.inspect(toolId);
      setInspectResults((prev) => ({ ...prev, [toolId]: ins }));
      setActionLogs((prev) => ({
        ...prev,
        [toolId]: res.restored ? `${t(locale, 'toolIntegrations.rollbackSuccess')} (${res.backupPath || ''})` : t(locale, 'toolIntegrations.rollbackFailed'),
      }));
    } catch (err) {
      setActionLogs((prev) => ({ ...prev, [toolId]: `${t(locale, 'toolIntegrations.rollbackFailed')}: ${String(err)}` }));
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-5xl mx-auto p-4">
      {/* Header Info */}
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] pb-4">
        <div>
          <h2 className="text-lg font-semibold text-[var(--text)] flex items-center gap-2">
            <Wrench className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'toolIntegrations.title')}
          </h2>
          <p className="text-xs text-[var(--text-secondary)] mt-1">
            {t(locale, 'toolIntegrations.desc')}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void runDetectAll()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs bg-[var(--surface-hover)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          {t(locale, 'toolIntegrations.rescan')}
        </button>
      </div>

      {/* Grid of Tool Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {TOOLS.map((tool) => {
          const detect = detectResults[tool.id];
          const inspect = inspectResults[tool.id];
          const patch = plannedPatches[tool.id];
          const log = actionLogs[tool.id];

          return (
            <div
              key={tool.id}
              className="flex flex-col justify-between p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] gap-4"
            >
              {/* Top Info */}
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Terminal className="w-4 h-4 text-[var(--primary)]" />
                    <span className="font-semibold text-sm text-[var(--text)]">{tool.name}</span>
                  </div>
                  <span
                    className={`flex items-center gap-1 text-[11px] px-2 py-0.5 rounded-full font-medium ${
                      detect?.installed
                        ? 'bg-[var(--success-soft)] text-[var(--success)]'
                        : 'bg-[var(--surface-hover)] text-[var(--text-disabled)]'
                    }`}
                  >
                    {detect?.installed ? <CheckCircle2 className="w-3 h-3" /> : <AlertCircle className="w-3 h-3" />}
                    {detect?.installed ? `${t(locale, 'toolIntegrations.installed')} ${detect.version || ''}` : t(locale, 'toolIntegrations.notInstalled')}
                  </span>
                </div>
                <p className="text-xs text-[var(--text-muted)]">{t(locale, tool.descKey)}</p>
              </div>

              {/* Status details */}
              <div className="flex flex-col gap-1.5 p-3 rounded-xl bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs">
                <div className="flex items-center justify-between">
                  <span className="text-[var(--text-secondary)]">{t(locale, 'toolIntegrations.configStatus')}</span>
                  <span className="font-medium text-[var(--text)]">
                    {inspect?.exists ? (inspect.managed ? t(locale, 'toolIntegrations.managed') : t(locale, 'toolIntegrations.exists')) : t(locale, 'toolIntegrations.notInit')}
                  </span>
                </div>
                {inspect?.configPath && (
                  <div className="flex items-center justify-between font-mono text-[10px] text-[var(--text-muted)] truncate">
                    <span>{t(locale, 'toolIntegrations.path')}</span>
                    <span className="truncate max-w-[240px]" title={inspect.configPath}>
                      {inspect.configPath}
                    </span>
                  </div>
                )}
                {inspect?.sensitiveKeys && inspect.sensitiveKeys.length > 0 && (
                  <div className="flex items-center gap-1 text-[10px] text-[var(--warning)] mt-0.5">
                    <Shield className="w-3 h-3" />
                    {t(locale, 'toolIntegrations.sensitiveDetected')} {inspect.sensitiveKeys.join(', ')}
                  </div>
                )}
              </div>

              {/* Plan Preview if generated */}
              {patch && (
                <div className="flex flex-col gap-1 p-3 rounded-lg border border-[var(--primary)]/30 bg-[var(--primary)]/5 text-xs">
                  <div className="flex items-center justify-between">
                    <span className="font-semibold text-[var(--text)] flex items-center gap-1">
                      <FileCode className="w-3.5 h-3.5 text-[var(--primary)]" />
                      {t(locale, 'toolIntegrations.planPreview')}
                    </span>
                    <span className="text-[10px] text-[var(--text-muted)]">{patch.summary}</span>
                  </div>
                  <pre className="text-[10px] font-mono text-[var(--text-secondary)] p-2 rounded bg-[var(--surface)] max-h-24 overflow-y-auto mt-1">
                    {patch.patchJson}
                  </pre>
                </div>
              )}

              {/* Action Log / Feedback */}
              {log && (
                <div className="text-[11px] p-2.5 rounded-lg bg-[var(--surface-hover)] text-[var(--text)] font-mono">
                  {log}
                </div>
              )}

              {/* 7-Step Action Buttons */}
              <div className="flex items-center flex-wrap gap-2 pt-2 border-t border-[var(--border-subtle)]">
                <button
                  type="button"
                  onClick={() => void handleBackup(tool.id)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs rounded border border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-colors"
                >
                  <Shield className="w-3.5 h-3.5" />
                  {t(locale, 'toolIntegrations.backup')}
                </button>
                <button
                  type="button"
                  onClick={() => void handlePlan(tool.id)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs rounded border border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-colors"
                >
                  <FileCode className="w-3.5 h-3.5" />
                  {t(locale, 'toolIntegrations.plan')}
                </button>
                <button
                  type="button"
                  onClick={() => void handleApply(tool.id)}
                  disabled={!patch}
                  className="flex items-center gap-1 px-3 py-1 text-xs rounded bg-[var(--primary)] text-[var(--primary-foreground)] font-medium hover:opacity-90 transition-opacity disabled:opacity-50"
                >
                  <PlayCircle className="w-3.5 h-3.5" />
                  {t(locale, 'toolIntegrations.apply')}
                </button>
                <button
                  type="button"
                  onClick={() => void handleVerify(tool.id)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs rounded border border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-colors"
                >
                  <FileCheck className="w-3.5 h-3.5" />
                  {t(locale, 'toolIntegrations.verify')}
                </button>
                <button
                  type="button"
                  onClick={() => void handleRollback(tool.id)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs rounded border border-[var(--border-subtle)] text-[var(--text-muted)] hover:text-[var(--danger)] hover:border-[var(--danger)] transition-colors ml-auto"
                >
                  <RotateCcw className="w-3.5 h-3.5" />
                  {t(locale, 'toolIntegrations.rollback')}
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
