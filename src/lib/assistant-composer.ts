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

export function mimeTypeFromPath(path: string): string {
  const extension = fileNameFromPath(path).split('.').at(-1)?.toLowerCase();
  return extension === 'json' ? 'application/json'
    : extension === 'md' ? 'text/markdown'
      : extension === 'ts' || extension === 'tsx' ? 'text/typescript'
        : extension === 'js' || extension === 'jsx' ? 'text/javascript'
          : extension === 'css' ? 'text/css'
            : extension === 'html' ? 'text/html'
              : 'text/plain';
}

export function assistantRetryPrompt(blocks: Array<{ type: string; content: unknown }>): string {
  return blocks.map(block => {
    const content = (block.content ?? {}) as Record<string, unknown>;
    if (block.type === 'text') return String(content.text ?? content.content ?? '');
    if (block.type === 'file_reference') return `[Attached file: ${String(content.path ?? '')}]`;
    return '';
  }).filter(Boolean).join('\n');
}
