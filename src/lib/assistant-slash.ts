/**
 * Assistant slash-command helpers (composer-local).
 *
 * Native currently does not expose executable slash commands.
 * Detection is kept pure so MessageInput can open an honest empty menu
 * without inventing capabilities or intercepting free-form `/text` as RPC.
 */

export type SlashCommandCategory = 'system' | 'skill' | 'mcp';

export interface SlashCommand {
  id: string;
  label: string;
  description: string;
  category: SlashCommandCategory;
  icon?: string;
}

export interface SlashDetection {
  /** Whether a slash-command menu should be open */
  active: boolean;
  /** Text after the leading `/` on the current line (no spaces) */
  query: string;
  /** Index of the leading `/` in the full value, or -1 */
  slashIndex: number;
}

/**
 * Detect slash input for the current line.
 *
 * Active when the last `/` on the text up to the caret is at line start
 * (document start or immediately after `\n`), and the fragment after it
 * contains neither space nor newline.
 *
 * Mid-sentence `/` (e.g. `see path/to/file`) does not activate.
 */
export function detectSlashInput(value: string, caret?: number): SlashDetection {
  const upto = caret === undefined ? value : value.slice(0, Math.max(0, Math.min(caret, value.length)));
  const slashIndex = upto.lastIndexOf('/');
  if (slashIndex < 0) {
    return { active: false, query: '', slashIndex: -1 };
  }

  const atLineStart = slashIndex === 0 || upto[slashIndex - 1] === '\n';
  if (!atLineStart) {
    return { active: false, query: '', slashIndex: -1 };
  }

  const afterSlash = upto.slice(slashIndex + 1);
  if (afterSlash.includes(' ') || afterSlash.includes('\n')) {
    return { active: false, query: '', slashIndex: -1 };
  }

  return { active: true, query: afterSlash, slashIndex };
}

/**
 * Runtime-provided slash commands.
 * Native has none today — keep an empty list rather than hardcoding fakes.
 */
export function listSlashCommands(): SlashCommand[] {
  return [];
}

export function filterSlashCommands(commands: SlashCommand[], query: string): SlashCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  return commands.filter(
    (cmd) =>
      cmd.id.toLowerCase().includes(q) ||
      cmd.label.toLowerCase().includes(q) ||
      cmd.description.toLowerCase().includes(q),
  );
}

/** Keys the composer must claim while the slash menu is open (no send). */
export function isSlashMenuKey(key: string): boolean {
  return key === 'Escape' || key === 'ArrowDown' || key === 'ArrowUp' || key === 'Enter';
}

export function nextSlashIndex(current: number, length: number, delta: 1 | -1): number {
  if (length <= 0) return 0;
  return (current + delta + length) % length;
}
