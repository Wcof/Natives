import { type AgentSession } from '../types/agent';

/**
 * 从 JSONL 行中提取会话标题（首条 human 消息，截取 100 字符）
 */
export function parseSessionTitle(lines: string[]): string {
  for (const line of lines) {
    try {
      const event = JSON.parse(line);
      if (event.type === 'human' && event.message?.content?.[0]?.text) {
        const title = event.message.content[0].text;
        return title.length > 100 ? title.slice(0, 100) : title;
      }
    } catch {
      continue;
    }
  }
  return 'Untitled';
}

/**
 * 解析 Claude Code 会话文件
 */
export function parseClaudeSessionFile(
  sessionId: string,
  projectPath: string,
  lines: string[],
): AgentSession {
  const filesModified = new Set<string>();
  const skillsUsed = new Set<string>();
  const title = parseSessionTitle(lines);

  for (const line of lines) {
    try {
      const event = JSON.parse(line);

      if (event.type === 'tool_use') {
        if (['Edit', 'Write', 'NotebookEdit'].includes(event.name)) {
          const filePath = event.input?.file_path;
          if (filePath) filesModified.add(filePath);
        }

        if (event.name === 'Skill') {
          const skill = event.input?.skill;
          if (skill) skillsUsed.add(skill);
        }
      }
    } catch {
      continue;
    }
  }

  return {
    id: sessionId,
    engine: 'claude',
    projectPath,
    title,
    startTime: Date.now(),
    filesModified: [...filesModified],
    skillsUsed: [...skillsUsed],
  };
}
