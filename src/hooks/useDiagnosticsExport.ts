'use client';

// useDiagnosticsExport（UX-12 · W8）— workbench 会话头部的诊断导出编排。
//
// 收集白名单来源：Host 只读 engine diagnostics + Run 元数据（不含内容）+
// 连接/就绪 + 有限脱敏日志 + 版本，经 lib/diagnostics-export 保存到本地。
// 不读取 prompt/message/tool I/O；导出文件本地保存不上传。

import { useCallback, useState } from 'react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import { useToast } from '@/lib/toast-context';
import { classifyError } from '@/lib/error-classifier';
import { executionEngine } from '@/lib/tauri/execution-engine';
import type { Run } from '@/lib/assistant-protocol';
import {
  saveDiagnosticsExport,
  type DiagnosticsRunInfo,
} from '@/lib/diagnostics-export';

export interface UseDiagnosticsExportInput {
  locale: Locale;
  /** 当前会话的 Run 元数据（白名单字段；无运行时传 null）。 */
  run: Run | null;
  /** 当前会话绑定项目路径（仅 surface 标识，不写绝对路径到导出）。 */
  projectPath?: string | null;
  /** 连接/就绪状态（来自 workspace store）。 */
  connection?: { connection?: string | null; reconnectAttempts?: number } | null;
  protocolVersion?: string | null;
  /** 覆盖 Host engine diagnostics（测试/降级用；默认实时读取）。 */
  engineDiagnostics?: Record<string, unknown> | null;
}

function toRunInfo(run: Run | null): DiagnosticsRunInfo | null {
  if (!run) return null;
  return {
    id: run.id,
    status: run.status ?? null,
    errorCode: run.errorCode ?? null,
    providerId: run.providerId ?? null,
    modelId: run.modelId ?? null,
    maxSteps: run.maxSteps ?? null,
    stepCount: run.stepCount ?? null,
    startedAt: run.startedAt ?? null,
    finishedAt: run.finishedAt ?? null,
  };
}

export function useDiagnosticsExport(input: UseDiagnosticsExportInput) {
  const { toast } = useToast();
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);

  const handleExport = useCallback(async () => {
    setExporting(true);
    setExportError(null);
    try {
      let engineDiagnostics: Record<string, unknown> | null = input.engineDiagnostics ?? null;
      if (!engineDiagnostics && executionEngine.getDiagnostics) {
        try {
          engineDiagnostics = (await executionEngine.getDiagnostics()) as Record<string, unknown>;
        } catch {
          engineDiagnostics = null; // Host diagnostics 不可用时不阻断导出
        }
      }
      let appVersion: string | null = null;
      if (window.nativesAPI?.app?.version) {
        try {
          appVersion = await window.nativesAPI.app.version();
        } catch {
          appVersion = null;
        }
      }
      const result = await saveDiagnosticsExport({
        locale: input.locale,
        appVersion,
        engineDiagnostics,
        run: toRunInfo(input.run),
        projectPath: input.projectPath ?? null,
        connection: input.connection ?? null,
      });
      if (result.ok === true && 'cancelled' in result) {
        // 用户取消保存对话框 → 静默（不算错误）
        return;
      }
      if (result.ok) {
        toast(t(input.locale, 'assistant.exportDiagnosticsSuccess'), 'success');
        return;
      }
      setExportError(result.error);
      toast(t(input.locale, 'assistant.exportDiagnosticsError'), 'error');
    } catch (err) {
      const classified = classifyError(err, { locale: input.locale });
      setExportError(classified.userMessage);
      toast(classified.userMessage, 'error');
    } finally {
      setExporting(false);
    }
  }, [input, toast]);

  return { exporting, exportError, handleExport };
}
