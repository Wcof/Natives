/**
 * Compatibility re-exports from the single protocol module.
 * Do not add parallel Conversation/Run/Event shapes here.
 */
export type { RunEvent as AssistantRunEvent, FileChange } from './assistant-protocol';

/** Shell runtime snapshot still uses path + change label. */
export interface AssistantFileChange {
  path: string;
  change: string;
  changeType?: string;
}
