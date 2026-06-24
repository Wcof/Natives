---
name: claude_rules
description: Custom workspace rules from CLAUDE.md, including mandatory rtk command prefixing and CodeGraph code graph navigation.
---

# CLAUDE.md Workspace Skill Rules

Please strictly adhere to the following rules loaded from [CLAUDE.md](file:///Users/ldh/Downloads/project/AiNative/Natives2/CLAUDE.md):

## 1. Mandatory RTK Command Prefix
All shell commands proposed or executed in this repository MUST be prefixed with `rtk`.
- **Correct**: `rtk npm run build`, `rtk git diff`, `rtk npm test`
- **Incorrect**: `npm run build`, `git diff`, `npm test`

## 2. CodeGraph Exploration First
Since the `.codegraph/` directory exists in the workspace root, code graph exploration must be used:
- Prioritize CodeGraph MCP tools (`codegraph_explore`, `codegraph_node`) or shell commands (`rtk codegraph explore "<query>"`, `rtk codegraph node <symbol>`) over standard grep/ripgrep or reading raw files when trying to locate definitions or understand call graphs.
