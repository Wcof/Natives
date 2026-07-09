---
name: help
description: Show Natives CLI help and available commands
---

# Natives Help

Show all available commands, agents, and skills in the current project.

## Steps

1. List all available commands from `.claude/commands/` and plugins
2. List all available agents from plugins
3. List all available skills from `.claude/skills/` and plugins
4. Show the current execution engine status (Native Runtime)

## Available Commands

- `/help` — Show this help
- `/status` — Show execution engine status

## Notes

Commands are loaded from `.claude/commands/` and plugin `commands/` directories.
Agents are loaded from plugin `agents/` directories.
Skills are loaded from `.claude/skills/` and plugin `skills/` directories.
