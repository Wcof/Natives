import type { ContentBlock } from '@/components/assistant/blocks';

export function formatElapsed(milliseconds: number): string {
  const seconds = Math.max(0, milliseconds) / 1000;
  if (seconds < 60) return `${(Math.round(seconds * 10) / 10).toFixed(1)}s`;
  return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
}

export function messagePlainText(blocks: ContentBlock[]): string {
  return blocks.filter(block => block.type === 'text').map(block => block.text ?? '').filter(Boolean).join('\n');
}
