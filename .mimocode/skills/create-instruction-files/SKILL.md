---
name: create-instruction-files
description: Analyze a codebase and create CLAUDE.md or AGENTS.md with high-signal, repo-specific guidance for future agents.
---

# Create Instruction Files

Generate CLAUDE.md or AGENTS.md for a repository. Every line must answer: "Would an agent likely miss this without help?"

## Process

### 1. Investigate (read highest-value sources first)

- `README*`, root manifests, workspace config, lockfiles
- Build, test, lint, formatter, typecheck, and codegen config
- CI workflows and pre-commit / task runner config
- Existing instruction files (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/`, `.cursorrules`, `.github/copilot-instructions.md`)
- Repo-local config such as `opencode.json`

If architecture is still unclear after reading config and docs, inspect a small number of representative code files to find real entrypoints, package boundaries, and execution flow.

Prefer executable sources of truth over prose. If docs conflict with config or scripts, trust the executable source.

### 2. Extract high-signal facts

Look for:
- Exact developer commands, especially non-obvious ones
- How to run a single test, a single package, or a focused verification step
- Required command order when it matters (e.g., `lint -> typecheck -> test`)
- Monorepo or multi-package boundaries, ownership of major directories
- Framework or toolchain quirks: generated code, migrations, codegen, build artifacts, special env loading
- Repo-specific style or workflow conventions that differ from defaults
- Testing quirks: fixtures, integration test prerequisites, required services
- Important constraints from existing instruction files worth preserving

### 3. Write the file

Include only:
- Exact commands and shortcuts the agent would otherwise guess wrong
- Architecture notes not obvious from filenames
- Conventions that differ from language or framework defaults
- Setup requirements, environment quirks, operational gotchas
- References to existing instruction sources that matter

Exclude:
- Generic software advice
- Long tutorials or exhaustive file trees
- Obvious language conventions
- Speculative claims or anything unverifiable

### 4. Validate

- Verify referenced file paths with Glob
- Verify referenced function/class names with Grep
- Run `npm run typecheck` or equivalent to ensure code references are correct

## Output

- For CLAUDE.md: Project overview, architecture, design philosophy, constraints, commands
- For AGENTS.md: Compact agent-focused guidance, commands, architecture, conventions, gotchas

Both should be short, practical, source-aware, and easy to validate.
