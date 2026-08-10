/**
 * tauri/types-module — Module domain 共享类型（ARCH-002 split）
 *
 * 模块写入（write_generated_module）等 wire payload 声明于此；
 * module facade（./module.ts）与业务组件从这里取类型。
 */

export interface WriteGeneratedModuleResult {
  moduleId: string;
  ok: boolean;
  oldContent?: string | null;
  newContent?: string;
  contractId?: string;
  contentHash?: string;
}
