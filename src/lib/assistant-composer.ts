export type AssistantPermissionProfile = 'readonly' | 'ask' | 'full_access';

export interface AssistantAttachment {
  path: string;
  name: string;
  mimeType: string;
  size: number;
}

export interface AssistantDraft {
  content: string;
  attachments: AssistantAttachment[];
}

export function canSendAssistantDraft(content: string, attachments: AssistantAttachment[]): boolean {
  return content.trim().length > 0 || attachments.length > 0;
}

export function normalizePermissionProfile(value: unknown): AssistantPermissionProfile {
  return value === 'readonly' || value === 'full_access' ? value : 'ask';
}

export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}
