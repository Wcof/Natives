---
name: "source-command-help"
description: "Show Natives CLI help and available commands"
---

# source-command-help

Use this skill when the user asks to run the migrated source command `help`.

## Command Template

# Natives Help

Show all available commands, agents, and skills in the current project.

## Steps

1. List all available commands from `.Codex/commands/` and plugins
2. List all available agents from plugins
3. List all available skills from `.Codex/skills/` and plugins
4. Show the current execution engine status (Native Runtime)

## Available Commands

- `/help` — Show this help
- `/status` — Show execution engine status

## Notes

Commands are loaded from `.Codex/commands/` and plugin `commands/` directories.
Agents are loaded from plugin `agents/` directories.
Skills are loaded from `.Codex/skills/` and plugin `skills/` directories.
