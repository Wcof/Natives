---
description: Show Natives execution engine status and configuration
---

# Engine Status

Show the current state of the Natives execution engine, including capabilities, hooks, rules, and plugins.

## Steps

1. Get the CapabilityRegistry status (how many capabilities, which are enabled)
2. Get the HookPipeline status (how many hooks, what types)
3. Get the RuleEngine status (how many rules loaded)
4. Get the PluginManager status (how many plugins loaded)
5. Display results in a structured format

## Notes

The Native Runtime uses the following architecture:
- **CapabilityRegistry**: Atomic capabilities (read_file, write_file, etc.)
- **HookPipeline**: PreToolUse / PostToolUse / Stop / UserPromptSubmit / SessionStart
- **RuleEngine**: .local.md rules for safety policies
- **PluginManager**: Claude Code-compatible plugin discovery
- **CASManager**: Commands, Agents, Skills
- **AgentLoop**: State machine with doom loop detection and self-heal
