/**
 * Agent Narration — parse terminal output to extract current agent action.
 * Based on fanbox's latestAgentAction() implementation.
 *
 * Detects Claude Code 2.x tool call format: ⏺ ToolName(args)
 * and other patterns like "esc to interrupt" for thinking state.
 */

const ACTION_VERB: Record<string, string> = {
  Read: 'Reading',
  Edit: 'Editing',
  Write: 'Writing',
  MultiEdit: 'Editing',
  Update: 'Editing',
  Bash: 'Running',
  Grep: 'Searching',
  Glob: 'Finding',
  Task: 'Task',
  TodoWrite: 'Planning',
  Fetch: 'Fetching',
  WebSearch: 'Searching',
  WebFetch: 'Fetching',
};

function baseOf(p: string): string {
  const parts = p.split('/');
  return parts[parts.length - 1] || p;
}

/**
 * Parse the last N lines of terminal output to find the current agent action.
 * @param lines Terminal output lines (most recent last)
 * @returns Human-readable action string, or empty if no action detected
 */
export function parseAgentAction(lines: string[]): string {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i] || '';

    // Web Search pattern
    const searchMatch = line.match(/(?:web\s*search|websearch)[\s(:]+[""]?([^"")\n]{1,40})/i);
    if (searchMatch) return `Searching ${searchMatch[1]?.trim() || ''}`;

    // Claude Code 2.x tool call: ⏺ ToolName(args)
    const toolMatch = line.match(/[⏺●·]\s*([A-Z][A-Za-z]+)\s*\(([^)]*)\)/);
    if (toolMatch) {
      const verb = ACTION_VERB[toolMatch[1]!] || toolMatch[1] || '';
      let arg = (toolMatch[2] || '').trim().replace(/^["']|["']$/g, '');
      // Shorten path arguments
      if (['Reading', 'Editing', 'Writing'].includes(verb) && arg.includes('/')) {
        arg = baseOf(arg);
      }
      if (arg.length > 30) arg = arg.slice(0, 30) + '…';
      return arg ? `${verb} ${arg}` : verb;
    }

    // Thinking state
    if (/esc to interrupt/i.test(line)) return 'Thinking…';
  }
  return '';
}

/**
 * Compose the narration bar text.
 * @param isActive Whether the bound agent is currently active
 * @param currentAction Parsed action from terminal output
 * @param currentFile The file currently being followed
 * @param isArtifact Whether the current file is a build artifact
 */
export function composeNarration(
  isActive: boolean,
  currentAction: string,
  currentFile: string | null,
  isArtifact: boolean,
): string {
  const fileName = currentFile ? baseOf(currentFile) : '';

  if (isActive && currentAction) return currentAction;
  if (isActive && fileName) return isArtifact ? `Generating ${fileName}` : `Editing ${fileName}`;
  if (!isActive && currentAction) return currentAction;
  if (!isActive && fileName) return `At ${fileName}`;
  return '';
}
