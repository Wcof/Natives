// Preview Service（T10 · C0 frozen）
//
// 编排：registry.candidates → provider.load → provider.prepare → PreviewModel。
// 规则：
// - 无全局 latest generation：stale/cancel 由 Surface 的 PreviewRequestController + signal 表达；
// - recoverable（not_applicable/unsupported/parse_failed）→ 记录 diagnostics 后尝试下一 provider；
// - fatal（permission_denied/security_violation/io_error/host_error）→ 立即停止并抛出；
// - cancelled（AbortSignal）→ 静默终止，不产生日志噪声。
// 本类近似无状态：不保存跨 Surface 的任何请求结果。

import type { PreviewContext, PreviewModel, PreviewRequest, PreviewProvider } from './contracts';
import { PreviewProviderError, cancelledError, fatalError } from './errors';
import type { PreviewRegistry } from './registry';
import type { PreviewDiagnostics } from './diagnostics';

export class PreviewService {
  constructor(
    private readonly registry: PreviewRegistry,
    private readonly ctx: PreviewContext,
    private readonly diagnostics?: PreviewDiagnostics,
  ) {}

  async prepare(request: PreviewRequest): Promise<PreviewModel> {
    const startedAt = Date.now();
    const candidates = this.registry.candidates(request);
    if (candidates.length === 0) {
      this.diagnostics?.record(request, 'no_provider', Date.now() - startedAt, undefined);
      return { kind: 'unsupported', reason: 'no preview provider matched' };
    }

    for (const entry of candidates) {
      if (request.signal?.aborted) {
        throw cancelledError();
      }
      let provider: PreviewProvider;
      try {
        provider = await entry.load();
      } catch (error) {
        // provider 模块加载失败视为 host_error（致命，不降级）
        throw fatalError('host_error', `failed to load preview provider ${entry.id}: ${String(error)}`);
      }
      try {
        const model = await provider.prepare(request, this.ctx);
        // provider 已返回但仍被 Surface 取消：丢弃 stale 结果，静默终止（Surface 已发起新请求）
        if (request.signal?.aborted) {
          throw cancelledError();
        }
        this.diagnostics?.record(request, entry.id, Date.now() - startedAt, model.kind);
        return model;
      } catch (error) {
        if (error instanceof PreviewProviderError) {
          if (error.recoverable) {
            this.diagnostics?.record(request, entry.id, Date.now() - startedAt, `recoverable:${error.code}`);
            continue;
          }
          throw error;
        }
        // 非分类异常 → host_error（致命）
        throw fatalError('host_error', `preview provider ${entry.id} failed: ${String(error)}`);
      }
    }

    this.diagnostics?.record(request, 'all_recoverable', Date.now() - startedAt, undefined);
    return { kind: 'unsupported', reason: 'all preview providers declined' };
  }
}
