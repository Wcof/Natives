// Preview Provider Registry（T10 · C0 frozen）
//
// 确定性注册表：priority 降序 + id 字典序稳定排序；重复 id 直接拒绝。
// builtin providers 也走统一 registry（T19 唯一组装），但本轮不开放第三方
// untrusted provider runtime。lazy provider 通过 ProviderLoader 按需加载。

import type { PreviewProvider, PreviewRequest, ProviderLoader } from './contracts';

interface Entry {
  id: string;
  priority: number;
  accepts: (request: PreviewRequest) => boolean;
  load: ProviderLoader;
}

export class PreviewRegistry {
  private entries: Entry[] = [];

  /** 注册 provider；重复 id 抛错（防止两个 provider 竞态处理同一 source） */
  register(provider: PreviewProvider | { id: string; priority: number; accepts: (r: PreviewRequest) => boolean; load: ProviderLoader }): void {
    if (this.entries.some((e) => e.id === provider.id)) {
      throw new Error(`duplicate preview provider: ${provider.id}`);
    }
    const entry: Entry = {
      id: provider.id,
      priority: provider.priority,
      accepts: provider.accepts,
      load: 'load' in provider ? provider.load : async () => provider as PreviewProvider,
    };
    this.entries.push(entry);
    this.entries.sort((a, b) => b.priority - a.priority || a.id.localeCompare(b.id));
  }

  /** 按 request 过滤出候选 provider（保持注册顺序的稳定排序） */
  candidates(request: PreviewRequest): Entry[] {
    return this.entries.filter((e) => e.accepts(request));
  }

  get size(): number {
    return this.entries.length;
  }
}
