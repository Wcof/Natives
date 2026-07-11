// ─── MCP Extension ───────────────────────────────────────
//
// MCP (Model Context Protocol) server support for the extension host.

export interface MCPServerConfig {
  id: string;
  name: string;
  transport: 'stdio' | 'http_sse';
  command?: string;
  args?: string[];
  env?: string[];
  url?: string;
}

export class MCPExtension {
  private servers: Map<string, MCPServerConfig> = new Map();

  register(config: MCPServerConfig): void {
    this.servers.set(config.id, config);
  }

  unregister(id: string): void {
    this.servers.delete(id);
  }

  list(): MCPServerConfig[] {
    return Array.from(this.servers.values());
  }

  get(id: string): MCPServerConfig | undefined {
    return this.servers.get(id);
  }
}

// ─── Skills Extension ────────────────────────────────────

export interface SkillDefinition {
  id: string;
  name: string;
  version: string;
  description: string;
  prompt: string;
  parameters?: Record<string, unknown>;
}

export class SkillsExtension {
  private skills: Map<string, SkillDefinition> = new Map();

  register(skill: SkillDefinition): void {
    this.skills.set(skill.id, skill);
  }

  unregister(id: string): void {
    this.skills.delete(id);
  }

  list(): SkillDefinition[] {
    return Array.from(this.skills.values());
  }

  get(id: string): SkillDefinition | undefined {
    return this.skills.get(id);
  }

  execute(id: string, params?: Record<string, unknown>): string {
    const skill = this.skills.get(id);
    if (!skill) throw new Error(`Skill '${id}' not found`);
    // In production, this would render the prompt with parameters
    return skill.prompt;
  }
}

// ─── Commands Extension ──────────────────────────────────

export interface CommandDefinition {
  name: string;
  description: string;
  parameters?: Array<{ name: string; type: string; required?: boolean; description?: string }>;
  handler: (params: Record<string, string>) => string;
}

export class CommandsExtension {
  private commands: Map<string, CommandDefinition> = new Map();

  register(cmd: CommandDefinition): void {
    this.commands.set(cmd.name, cmd);
  }

  unregister(name: string): void {
    this.commands.delete(name);
  }

  list(): CommandDefinition[] {
    return Array.from(this.commands.values());
  }

  execute(name: string, params: Record<string, string> = {}): string {
    const cmd = this.commands.get(name);
    if (!cmd) throw new Error(`Command '${name}' not found`);
    return cmd.handler(params);
  }
}

// ─── Hooks Extension ─────────────────────────────────────

export type HookPoint =
  | 'before_run' | 'after_run' | 'before_prompt' | 'after_prompt'
  | 'before_tool_call' | 'after_tool_call' | 'before_permission'
  | 'after_permission' | 'on_completion' | 'on_error';

export type HookAction = 'allow' | 'modify' | 'block' | 'prompt';

export interface HookDefinition {
  id: string;
  hookPoint: HookPoint;
  priority: number;
  handler: (context: unknown) => { action: HookAction; modified?: unknown; reason?: string };
  timeoutMs: number;
}

export class HooksExtension {
  private hooks: Map<string, HookDefinition> = new Map();

  register(hook: HookDefinition): void {
    this.hooks.set(hook.id, hook);
  }

  unregister(id: string): void {
    this.hooks.delete(id);
  }

  list(): HookDefinition[] {
    return Array.from(this.hooks.values()).sort((a, b) => b.priority - a.priority);
  }

  run(hookPoint: HookPoint, context: unknown): Array<{ id: string; action: HookAction; modified?: unknown; reason?: string }> {
    const results: Array<{ id: string; action: HookAction; modified?: unknown; reason?: string }> = [];
    const matching = Array.from(this.hooks.values())
      .filter(h => h.hookPoint === hookPoint)
      .sort((a, b) => b.priority - a.priority);

    for (const hook of matching) {
      const result = hook.handler(context);
      results.push({ id: hook.id, ...result });
      if (result.action === 'block') break; // Stop on block
    }
    return results;
  }
}